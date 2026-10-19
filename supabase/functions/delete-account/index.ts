// Supabase Edge Function: delete-account (ADR-0017, section 10, "Delete my account").
//
// The OpenBark website portal calls it with the signed-in person's own access
// token. The function asks Supabase Auth who that token belongs to, then
// deletes that user, and only that user, with the service role. The service
// role key is provided to the function by Supabase at run time; it is never in
// the repository and never reaches a browser.
//
// What goes: the auth user, and through `on delete cascade` their row in
// public.profiles. When the sync schema arrives (ADR-0017 section 11), its
// tables must also reference auth.users with `on delete cascade`, or be
// deleted here before the user, so this one call still removes every row and
// key envelope. Data on the person's own computers is never touched.
//
// Request:  POST, `Authorization: Bearer <the user's access token>`,
//           body {"confirm": "<the account's email address>"}.
//           The confirmation guards against an accidental or scripted call.
// Response: 200 {"deleted": true}, or 4xx/5xx {"error": "<code>"}.
//
// Deploy (the owner, once; see supabase/README.md):
//   supabase functions deploy delete-account --no-verify-jwt --project-ref nmecovtmijnfnjuadicp
// `--no-verify-jwt` because the project signs tokens with asymmetric (ES256)
// keys, and the function checks the token itself with Supabase Auth below.
import { createClient } from "@supabase/supabase-js"; // pinned to 2.118.0 in deno.json

// Browsers may call this only from the website. Calls without an Origin
// header (the desktop app, later) are allowed: the token is what authorises.
// For local testing: supabase secrets set PORTAL_ORIGINS=https://www.openbark.io,http://localhost:8099
const ORIGINS = (Deno.env.get("PORTAL_ORIGINS") ?? "https://www.openbark.io,https://openbark.io")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);

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
  // SUPABASE_SERVICE_ROLE_KEY is set by Supabase for every function. If the
  // project's legacy keys are ever turned off, set a secret key under this
  // second name instead: supabase secrets set OPENBARK_SECRET_KEY=sb_secret_...
  const serviceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") || Deno.env.get("OPENBARK_SECRET_KEY");
  if (!url || !serviceKey) return reply(500, { error: "not_configured" }, origin);

  const admin = createClient(url, serviceKey, {
    auth: { persistSession: false, autoRefreshToken: false, detectSessionInUrl: false },
  });

  // Who is asking? Supabase Auth checks the token's signature, expiry and
  // session; a publishable key or a forged token gets no user.
  const { data: who, error: whoError } = await admin.auth.getUser(token);
  const user = who?.user;
  if (whoError || !user) return reply(401, { error: "not_signed_in" }, origin);

  let body: { confirm?: unknown } = {};
  try {
    body = await req.json();
  } catch {
    // An empty or malformed body fails the confirmation below.
  }
  const typed = typeof body.confirm === "string" ? body.confirm.trim().toLowerCase() : "";
  if (!user.email || typed !== user.email.toLowerCase()) {
    return reply(400, { error: "confirmation_mismatch" }, origin);
  }

  const { error: deleteError } = await admin.auth.admin.deleteUser(user.id);
  if (deleteError) {
    // No email or profile in the log: the user id is enough to follow up.
    console.error(`delete-account: deleting ${user.id} failed: ${deleteError.message}`);
    return reply(500, { error: "delete_failed" }, origin);
  }
  return reply(200, { deleted: true }, origin);
});
