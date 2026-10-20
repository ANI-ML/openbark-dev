// Call errors in plain words (docs/plans/telephony.md, "Errors (calls)").
// Each names the cause and where to fix it. The strings are the English keys
// the phone screen translates. The codes are those the Voice SDK raises, as
// listed in @twilio/voice-sdk 2.18.5's errors/generated.ts.

export const CALL_ERRORS = {
  wrongKey:
    "Twilio didn't accept this key. Check the SID and secret, or make a new key in your Twilio console.",
  expired: "The phone's sign-in with Twilio ran out. Close this window and call again.",
  authorization: "Twilio didn't allow this call. In Settings > Phone calls, choose Check setup to see why.",
  appNotFound: "Twilio couldn't find OpenBark's TwiML App. In Settings > Phone calls, choose Change, then Connect.",
  account: "Twilio refused the call: your account needs attention. Check billing in your Twilio console.",
  rateLimited: "Twilio is getting too many requests from this account. Wait a minute and try again.",
  noAudio:
    "The call connected but no sound is getting through. Your network may be blocking Twilio's call audio. Ask your IT provider to allow UDP to Twilio (see the setup guide).",
  linux: "Calls work in web mode on Linux. Run `openbark serve`, then open OpenBark in Chrome or Firefox.",
  microphone: "OpenBark can't use the microphone. Allow it in your computer's privacy settings, then try again.",
  offline: "Can't reach Twilio. Check this computer's internet connection.",
  busy: "The line is busy.",
  noAnswer: "No answer.",
  declined: "The call was declined.",
  notFound: "That number couldn't be reached. Check it and try again.",
  dropped: "The call dropped. What was recorded is saved.",
  noWebrtc: "This browser can't make calls. Use a current version of Chrome, Edge, Firefox or Safari.",
} as const;

/** What a Twilio Voice SDK error, a browser error or an engine error means for the clinic. */
export function callErrorMessage(e: unknown): string {
  const err = (e ?? {}) as { code?: number; name?: string; message?: string };
  switch (typeof err.code === "number" ? err.code : undefined) {
    // The token was refused: a wrong or deleted key, or a bad signature.
    case 20101: // AccessTokenInvalid
    case 20151: // AuthenticationFailed
    case 31202: // AccessTokenSignatureValidationFailed
    case 31204: // InvalidJWTTokenError
      return CALL_ERRORS.wrongKey;
    case 20104: // AccessTokenExpired
    case 31205: // JWTTokenExpiredError
      return CALL_ERRORS.expired;
    case 31201: // AuthorizationError
      return CALL_ERRORS.authorization;
    case 31001: // ApplicationNotFoundError
      return CALL_ERRORS.appNotFound;
    case 31203: // NoValidAccountError
      return CALL_ERRORS.account;
    case 31206: // RateExceededError
      return CALL_ERRORS.rateLimited;
    case 31401: // PermissionDeniedError (the microphone)
    case 31402: // AcquisitionFailedError (the microphone)
      return CALL_ERRORS.microphone;
    case 31003: // ConnectionTimeoutError
    case 31005: // ConnectionError (signalling)
    case 31009: // TransportError
    case 53000: // ConnectionError
    case 53001: // ConnectionDisconnected
      return CALL_ERRORS.offline;
    case 53405: // media connection failed: usually UDP blocked
      return CALL_ERRORS.noAudio;
    case 31486: // BusyHere
      return CALL_ERRORS.busy;
    case 31480: // TemporarilyUnavailable
      return CALL_ERRORS.noAnswer;
    case 31603: // Decline
      return CALL_ERRORS.declined;
    case 31404: // NotFound
      return CALL_ERRORS.notFound;
  }
  if (err.name === "NotAllowedError" || err.name === "NotFoundError" || err.name === "SecurityError") {
    return CALL_ERRORS.microphone;
  }
  if (err.message) return err.message;
  return typeof e === "string" ? e : String(e);
}
