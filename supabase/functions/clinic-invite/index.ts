// Supabase Edge Function: clinic-invite (ADR-0028, "Invites by email").
//
// A clinic admin on the website portal invites people by email. The function
// asks Supabase Auth who the token belongs to, then calls
// clinic.invite_members_for(user, clinic, emails, role) with the service role.
// That SQL function checks that the caller is an admin of the clinic, applies
// the daily limit (50 invites and join codes per clinic), and records one
// invite per address, keyed by the lower-cased email, for 14 days.
//
// Then, for each address Supabase Auth doesn't know yet (or invited before
// but who never signed in), it sends Supabase Auth's invite email
// (inviteUserByEmail, template supabase/templates/clinic-invite.html). That
// email has no sign-in link: it points at the account page, where the person
// signs in with an emailed code and accepts. People who already have an
// account get no email: the portal tells the admin they'll see the invite
// when they next sign in. Nothing clinical is involved: an email address, a
// role and a clinic id.
//
// Request:  POST, `Authorization: Bearer <the admin's access token>`,
//           body {"clinic_id": "<uuid>", "emails": ["a@b.c", ...] (1 to 50), "role": "member"|"admin"}.
// Response: 200 {"results": [{"email", "status": invited|renewed|member|invalid,
//                             "delivery": emailed|sign_in|email_failed|none}]},
//           or 4xx/5xx {"error": "<code>"}.
//
// Deploy (the owner, with the clinic migration; see supabase/README.md):
//   supabase functions deploy clinic-invite --no-verify-jwt --project-ref nmecovtmijnfnjuadicp
import { createClient } from "@supabase/supabase-js"; // pinned to 2.118.0 in deno.json

const ORIGINS = (Deno.env.get("PORTAL_ORIGINS") ?? "https://www.openbark.io,https://openbark.io")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);

// Where the invite email's button would land if a template ever carries a
// link; it must be in Authentication > URL Configuration > Redirect URLs.
const PORTAL_URL = Deno.env.get("PORTAL_URL") ?? "https://www.openbark.io/portal";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

type Result = { email: string; status: string; account?: string; delivery?: string };

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
    db: { schema: "clinic" },
  });
  // Who is asking? A publishable key or a forged token gets no user.
  const { data: who, error: whoError } = await admin.auth.getUser(token);
  const user = who?.user;
  if (whoError || !user) return reply(401, { error: "not_signed_in" }, origin);

  let body: { clinic_id?: unknown; emails?: unknown; role?: unknown } = {};
  try {
    body = await req.json();
  } catch {
    // Fails the checks below.
  }
  const clinicId = typeof body.clinic_id === "string" ? body.clinic_id.trim() : "";
  if (!UUID.test(clinicId)) return reply(400, { error: "bad_clinic" }, origin);
  const role = body.role === "admin" ? "admin" : body.role === "member" || body.role === undefined ? "member" : "";
  if (!role) return reply(400, { error: "bad_role" }, origin);
  if (!Array.isArray(body.emails) || body.emails.length === 0) return reply(400, { error: "no_emails" }, origin);
  if (body.emails.length > 50) return reply(400, { error: "too_many" }, origin);
  const emails = body.emails.map((e) => (typeof e === "string" ? e.slice(0, 320) : ""));

  // The SQL function checks the admin role, the limit and each address.
  const { data, error } = await admin.rpc("invite_members_for", {
    user_id: user.id,
    clinic_id: clinicId,
    emails,
    role,
  });
  if (error) {
    const code = (error.message ?? "").startsWith("openbark:") ? error.message.slice(9) : "invite_failed";
    if (code === "invite_failed") console.error(`clinic-invite: ${user.id} ${clinicId}: ${error.message}`);
    const status = code === "not_admin" ? 403 : code === "rate_limited" ? 429 : code === "invite_failed" ? 500 : 400;
    return reply(status, { error: code }, origin);
  }

  const results: Result[] = Array.isArray(data) ? data : [];
  for (const r of results) {
    if (r.status !== "invited" && r.status !== "renewed") {
      r.delivery = "none";
    } else if (r.account === "active") {
      r.delivery = "sign_in";
    } else {
      // New to Supabase Auth, or invited before and never signed in: send
      // (or send again) the invite email. An address that turns out to have
      // signed up meanwhile sees the invite at sign-in instead.
      const { error: sendError } = await admin.auth.admin.inviteUserByEmail(r.email, { redirectTo: PORTAL_URL });
      if (!sendError) {
        r.delivery = "emailed";
      } else if ((sendError as { code?: string }).code === "email_exists" || sendError.status === 422) {
        r.delivery = "sign_in";
      } else {
        // No address in the log: the clinic id and the error are enough.
        console.error(`clinic-invite: ${clinicId}: invite email failed: ${sendError.message}`);
        r.delivery = "email_failed";
      }
    }
    delete r.account;
  }
  return reply(200, { results }, origin);
});
