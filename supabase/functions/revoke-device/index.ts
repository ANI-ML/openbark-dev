// Supabase Edge Function: revoke-device (ADR-0017 section 3, "Removing a device").
//
// The website portal can remove a device from a person's sync account. The
// portal holds no keys and no device session, so it can't call the sync
// functions a device calls (they require an approved device's session).
// This function asks Supabase Auth who the token belongs to, then calls
// sync.revoke_device_for(user, device) with the service role, for that user
// only. The device is refused at once (row level security), its auth session
// is deleted, and the account is marked so the next approved device to come
// online rotates the keys. The function never sees anything clinical: the
// sync schema holds only ciphertext, and this touches only the devices row.
//
// The desktop app removes devices itself (sync.revoke_device over REST); this
// function is for the portal.
//
// Request:  POST, `Authorization: Bearer <the user's access token>`,
//           body {"device_id": "<uuid>"}.
// Response: 200 {"revoked": "<uuid>"}, or 4xx/5xx {"error": "<code>"}.
//
// Deploy (the owner, with the sync migration; see supabase/README.md):
//   supabase functions deploy revoke-device --no-verify-jwt --project-ref nmecovtmijnfnjuadicp
import { createClient } from "@supabase/supabase-js"; // pinned to 2.118.0 in deno.json

const ORIGINS = (Deno.env.get("PORTAL_ORIGINS") ?? "https://www.openbark.io,https://openbark.io")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function cors(origin: string | null): Record<string, string> {
  const headers: Record<string, string> = { Vary: "Origin" };
  if (origin && ORIGINS.includes(origin)) {
    headers["Access-Control-Allow-Origin"] = origin;
    headers["Access-Control-Allow-Methods"] = "POST, OPTIONS";
    headers["Access-Control-Allow-Headers"] = "authorization, apikey, content-type, x-client-info";
    headers["Access-Control-Max-Age"] = "600";
  }
  return headers;
}

function reply(status: number, body: Record<string, unknown>, origin: string | null): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...cors(origin), "Content-Type": "application/json", "Cache-Control": "no-store" },
  });
}

Deno.serve(async (req: Request): Promise<Response> => {
  const origin = req.headers.get("Origin");
  if (req.method === "OPTIONS") return new Response(null, { status: 204, headers: cors(origin) });
  if (req.method !== "POST") return reply(405, { error: "method_not_allowed" }, origin);
  if (origin && !ORIGINS.includes(origin)) return reply(403, { error: "origin_not_allowed" }, origin);

  const header = req.headers.get("Authorization") ?? "";
  const token = /^Bearer\s+/i.test(header) ? header.replace(/^Bearer\s+/i, "").trim() : "";
  if (!token) return reply(401, { error: "not_signed_in" }, origin);

  const url = Deno.env.get("SUPABASE_URL");
  const serviceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") || Deno.env.get("OPENBARK_SECRET_KEY");
  if (!url || !serviceKey) return reply(500, { error: "not_configured" }, origin);

  const admin = createClient(url, serviceKey, {
    auth: { persistSession: false, autoRefreshToken: false, detectSessionInUrl: false },
    db: { schema: "sync" },
  });
  const { data: who, error: whoError } = await admin.auth.getUser(token);
  const user = who?.user;
  if (whoError || !user) return reply(401, { error: "not_signed_in" }, origin);

  let body: { device_id?: unknown } = {};
  try {
    body = await req.json();
  } catch {
    // Fails the check below.
  }
  const deviceId = typeof body.device_id === "string" ? body.device_id.trim() : "";
  if (!UUID.test(deviceId)) return reply(400, { error: "bad_device" }, origin);

  // The SQL function checks the device belongs to this user.
  const { data, error } = await admin.rpc("revoke_device_for", { user_id: user.id, device_id: deviceId });
  if (error) {
    const code = (error.message ?? "").startsWith("openbark:") ? error.message.slice(9) : "revoke_failed";
    if (code === "revoke_failed") console.error(`revoke-device: ${user.id} ${deviceId}: ${error.message}`);
    return reply(code === "unknown_device" ? 404 : 500, { error: code }, origin);
  }
  return reply(200, data ?? { revoked: deviceId }, origin);
});
