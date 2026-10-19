// Call errors in plain words (docs/plans/telephony.md, "Errors (calls)").
// Each names the cause and where to fix it. The strings are the English keys
// the phone screen translates.

export const CALL_ERRORS = {
  wrongKey:
    "Twilio didn't accept this key. Check the SID and secret, or make a new key in your Twilio console.",
  trial:
    "Your Twilio account is a trial, and trials can't connect calls through OpenBark. Upgrade it in your Twilio console.",
  noAudio:
    "The call connected but no sound is getting through. Your network may be blocking Twilio's call audio. Ask your IT provider to allow UDP to Twilio (see the setup guide).",
  linux: "Calls work in web mode on Linux. Run `openbark serve`, then open OpenBark in Chrome or Firefox.",
  geo: "Twilio blocked a call to this country. Turn it on in your Twilio console under Voice > Settings > Geo permissions.",
  microphone: "OpenBark can't use the microphone. Allow it in your computer's privacy settings, then try again.",
  offline: "Can't reach Twilio. Check this computer's internet connection.",
  dropped: "The call dropped. What was recorded is saved.",
  account: "Twilio refused the call: your account needs attention. Check billing in your Twilio console.",
  noWebrtc: "This browser can't make calls. Use a current version of Chrome, Edge, Firefox or Safari.",
} as const;

/** What a Twilio Voice SDK error, a browser error or an engine error means for the clinic. */
export function callErrorMessage(e: unknown): string {
  const err = (e ?? {}) as { code?: number; name?: string; message?: string };
  const code = typeof err.code === "number" ? err.code : undefined;
  switch (code) {
    // Access token problems: the key or secret was wrong, or the key was revoked.
    case 20101:
    case 20103:
    case 20104:
    case 20151:
    case 20157:
    case 31202:
    case 31204:
    case 31205:
    case 20003:
      return CALL_ERRORS.wrongKey;
    case 21219:
    case 21210:
      return CALL_ERRORS.trial;
    case 21215:
    case 13227:
    case 32205:
      return CALL_ERRORS.geo;
    case 20005:
    case 31402:
      return CALL_ERRORS.account;
    // The microphone was refused or is missing.
    case 31208:
    case 31401:
    case 31201:
      return CALL_ERRORS.microphone;
    // Signalling couldn't reach Twilio.
    case 31005:
    case 31009:
    case 53000:
    case 53001:
      return CALL_ERRORS.offline;
    // Media couldn't connect: usually UDP blocked by a firewall.
    case 31003:
    case 53405:
      return CALL_ERRORS.noAudio;
  }
  if (err.name === "NotAllowedError" || err.name === "NotFoundError" || err.name === "SecurityError") {
    return CALL_ERRORS.microphone;
  }
  if (err.message) return err.message;
  return typeof e === "string" ? e : String(e);
}
