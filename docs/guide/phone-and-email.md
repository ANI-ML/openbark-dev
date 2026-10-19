# Phone calls (and, later, email)

OpenBark can phone a client through **your clinic's own Twilio account**, record the call on this computer, and turn it into a visit with a **Phone call** note. You sign up with Twilio and pay Twilio directly. ANI.ML Health runs nothing for this, never sees your keys, your calls or your bills, and is paid nothing.

This release makes **outgoing calls**. Taking calls, and emailing documents through your own SendGrid account, come in later releases.

The design is in [ADR-0023](../adr/0023-bring-your-own-telephony.md); what goes over the network is in [docs/network.md](../network.md) (R9 and R10).

## Before you start

- **You pay Twilio directly.** Twilio bills you for your number (if you buy one) and for each minute of calling. Check Twilio's own price pages for your country: https://www.twilio.com/en-us/voice/pricing. OpenBark never shows a price as fact.
- **Check your local rules on recording calls.** OpenBark announces every recorded call and asks whether the client agrees, but your association or lawyer decides whether that is enough where your clients live. A client may be somewhere that needs everyone's agreement, so OpenBark announces every recorded call whatever your clinic's own rule is.
- **Twilio has no Canadian data centre.** Twilio keeps call logs (numbers, times, durations) in your account in the United States, and the call's audio passes through Twilio's servers while the call lasts, as with any phone company. The recording itself is made and kept on your computer: OpenBark never asks Twilio to record.
- **Call-quality figures.** During a call, Twilio's calling software sends call-quality figures (call state, round-trip time, jitter, packet loss; no audio) to your own Twilio account. Twilio's software has no public switch to turn this off.
- **Firewalls.** Your network must let this computer send UDP to Twilio (168.86.128.0/18, ports 10000 to 60000). Most clinic networks already do. If calls connect but nobody can hear anything, ask your IT provider.
- **Which computers.** Windows: the desktop app. Mac: the desktop app should work but hasn't been tested on a real Mac yet; if it doesn't, use web mode. Linux: the desktop app can't make calls (its web engine has no WebRTC); run `openbark serve` and use OpenBark in Chrome or Firefox.

## Part 1: in your Twilio console (about 15 minutes)

1. **Create a Twilio account and upgrade it** (add a payment method and a starting balance). A trial account can't connect calls through OpenBark: Twilio strips the call instruction OpenBark uses from trial accounts.
2. **Choose your caller ID**, the number clients see:
   - buy a number (Phone Numbers > Buy a number); or
   - verify your clinic's existing number (Phone Numbers > Verified Caller IDs). Clients who call back reach your usual phones.
3. **Limit where calls can go:** Voice > Settings > Geo permissions. Turn on only the countries you call, normally Canada and the United States. This limits the damage if a key ever leaks.
4. **Set a usage alert:** Billing > Usage triggers. For example, an email when you pass 50 in a month.
5. **Make an API key for this computer:** Account > API keys & tokens > Create API key.
   - Region: **United States (US1)**.
   - Type: **Standard**. A Restricted key can't be used for calls.
   - Name it after the computer, for example "OpenBark front desk". One key per computer, so you can revoke one without touching the others.
   - Copy the **SID** (it starts with SK) and the **secret** now: Twilio shows the secret only once.
6. **Make a TwiML Bin named "OpenBark outgoing":** TwiML Bins > Create. In OpenBark, open the Connect Twilio sheet (Part 2), type your caller ID and choose **Show the Twilio text**. Copy that text into the Bin, save it, and copy the Bin's URL (it starts with `https://handler.twilio.com/twiml/`). Twilio has no way for OpenBark to make the Bin for you. The text asks Twilio to dial the client from your caller ID and to record nothing:

   ```xml
   <Response>
     <Dial callerId="+15555550100" answerOnBridge="true" timeLimit="3600">
       <Number>{{#e164}}{{To}}{{/e164}}</Number>
     </Dial>
   </Response>
   ```

## Part 2: in OpenBark

Only a veterinarian can connect or disconnect Twilio, or change the phone settings. Anyone on the team can make calls.

1. **Settings > Phone calls > Connect Twilio.**
2. Paste your **Account SID** (on your Twilio console's home page, it starts with AC), the **API key SID** and the **API key secret**.
3. Give **this computer's name**: a place, not a person, such as `front-desk`.
4. Type your **caller ID** and paste the **Outgoing Bin URL**.
5. Choose **Connect**. OpenBark checks the key with Twilio, checks that the caller ID belongs to your account, and makes a TwiML App called "OpenBark" that uses your Bin. Only then does it save anything: the secret in this computer's keychain (or, without a keychain, in a file sealed with OpenBark's encryption key), and the rest in OpenBark's settings. After saving, the secret is shown only as its last four characters.
6. Turn on **Turn on the phone on this computer**.
7. Check **Record calls** and the **Announcement**. The default is: "This call is recorded for your pet's medical record. The recording stays at the clinic. Is that all right with you?" Recorded calls always start with it.
8. Choose **Check setup**. Each line says what is right, or what is wrong and where to fix it in your Twilio console. It always reminds you that real calls need an upgraded account, because OpenBark's key can't read whether your account is a trial.

To change the caller ID or the Bin later, choose **Change**. You don't need to paste the secret again for the same key.

## Making a call

1. Open a patient and choose **Call**, or choose **Call** on a visit. **Patients > Call a number** opens the dial pad. To call a client from their patient, add the **Owner's phone** on the patient (**Edit details**).
2. The phone window opens. It says **This call will be recorded and announced.** Tick **Don't record this call** to make this one call without recording.
3. Choose **Call**. You hear ringing until the client answers.
4. When the client answers, recording starts and the window shows the announcement: **Read this to the client now:**. Read it, then choose **I've read it**.
5. OpenBark asks **Did the client agree?**
   - **Yes, keep recording**: the recording goes on; the visit says the client agreed.
   - **No, stop recording**: what was recorded is deleted, and the call goes on unrecorded. The visit says the client declined, and when.
   - If you don't answer, the recording goes on, and the visit says the client stayed on the line after the announcement.
6. During the call: **Mute**, **Hold** (neither of you hears the other), **Keypad** (for phone menus), and **Stop and delete the recording** at any time.
7. **Hang up.** A recorded call is saved as a visit, opened in the main window with the **Phone call** template chosen, and drafted if **Draft the note when I stop recording** is on.

A call you don't record saves nothing.

## The recording and the visit

- Both sides are recorded on this computer, each on its own channel, encrypted as they are written, like any visit.
- The transcript labels the speakers by channel, so they are exact: **Vet** or **Tech** (the role of the team member who called) or **Clinic** without a team, and **Client**. OpenBark computes no voice prints on calls.
- Review shows **Phone call: Outgoing · 4:12 · +1 555 555 0142**, and the consent line, for example **Announcement read by Dr Lee at 0:00. Client agreed.**
- The visit keeps Twilio's call SID, so you can match it with your Twilio call logs.
- Drafting, review, signing, PDFs and copying work as for any visit.

## If something goes wrong

| Message | What it means | What to do |
|---|---|---|
| Twilio didn't accept this key. | The key SID or secret is wrong, or the key was deleted | Check both, or make a new Standard key (Part 1, step 5) and connect again |
| Your Twilio account is a trial, and trials can't connect calls through OpenBark. | The account isn't upgraded | Upgrade it in your Twilio console |
| … isn't one of this Twilio account's numbers or verified caller IDs. | The caller ID isn't yours in Twilio | Buy it or verify it (Part 1, step 2), or choose another |
| The … Bin URL should be the Bin's own URL … | A console page's address was pasted | Copy the URL shown on the Bin's own page |
| Twilio blocked a call to this country. | Geo permissions exclude that country | Voice > Settings > Geo permissions |
| Twilio refused the call: your account needs attention. | Suspended, closed, or out of funds | Check billing in your Twilio console |
| The call connected but no sound is getting through. | The network blocks Twilio's call audio (UDP) | Ask your IT provider to allow UDP to 168.86.128.0/18, ports 10000 to 60000 |
| Can't reach Twilio. | No internet, or a firewall blocks Twilio | Check this computer's connection |
| OpenBark can't use the microphone. | The microphone permission was refused | Allow it in the computer's privacy settings, or in the browser's address bar in web mode |
| Allow pop-ups for this page to make calls. | Web mode: the browser blocked the phone window | Allow pop-ups for OpenBark's page |
| Calls work in web mode on Linux. | The Linux desktop app can't make calls | Run `openbark serve` and use Chrome or Firefox |
| The call dropped. What was recorded is saved. | The connection was lost | Call again; the visit keeps what was recorded |
| No answer. | The call ended before anyone answered | Try again later |

## Turning it off

**Settings > Phone calls > Disconnect** removes the key from this computer and turns the phone off. Then delete the key in your Twilio console as well (Account > API keys & tokens), so it can't be used anywhere else. Your announcement wording is kept.

## Other computers

Make one Standard key per computer (Part 1, step 5) and connect each computer with its own key and name. They can share the same Bin.

## Email (later)

Emailing discharge instructions and summaries through your clinic's own SendGrid account is planned for a later release ([ADR-0023](../adr/0023-bring-your-own-telephony.md), section 9).
