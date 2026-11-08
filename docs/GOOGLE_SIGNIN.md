# Sign in with Google (sync through Google Drive)

Kakangkung signs in with a Google account and keeps sync data (bookmarks, history, open tabs,
settings, extension list) in that account's Google Drive, in the hidden **app data folder**:
only Kakangkung can see it, and Kakangkung cannot see the person's own Drive files.

## How it works

- Sign-in is OAuth 2.0 for installed apps (RFC 8252), the flow Google requires for desktop apps:
  the consent page opens in the **default browser**, Google redirects to a one-shot listener on
  `127.0.0.1`, and the code is exchanged with PKCE (S256).
- Scopes: `openid email profile https://www.googleapis.com/auth/drive.appdata`.
- The refresh token is stored DPAPI-encrypted (`google.token` in the settings database); only the
  same Windows user on the same PC can decrypt it. Sign out deletes it and revokes it at Google.
- Sync itself is unchanged (`storage::sync` merges one file per device). For a Google account the
  sync folder is `%LOCALAPPDATA%\Kakangkung\Google Drive\account-…`, and a mirror thread keeps it
  equal to the Drive app data folder (uploads this PC's file, downloads the other PCs' files)
  every 2 minutes and right after each sync.

Code: `crates/app/src/google.rs`; DPAPI in `crates/platform/src/secret.rs`.

## Setting up the OAuth client (once, by whoever builds Kakangkung)

Google gives every app its own client id; Kakangkung cannot ship one for you.

1. Open <https://console.cloud.google.com/>, create (or pick) a project.
2. **APIs & Services → Library**: enable **Google Drive API**.
3. **APIs & Services → OAuth consent screen** (Google Auth Platform): app name "Kakangkung",
   user type **External**; under **Data access** add the scopes
   `openid`, `.../auth/userinfo.email`, `.../auth/userinfo.profile`, `.../auth/drive.appdata`.
   While the app is in **Testing**, add each Google account that will sign in under **Audience →
   Test users** (up to 100). Publishing the app removes that limit.
4. **Clients → Create client → Application type: Desktop app**. Download the JSON.
5. Give it to Kakangkung, one of:
   - save the downloaded file as `%LOCALAPPDATA%\Kakangkung\google-client.json`; or
   - set `KAKANGKUNG_GOOGLE_CLIENT_ID` and `KAKANGKUNG_GOOGLE_CLIENT_SECRET` in the environment
     (when set while running `cargo build`, they are built into the program).

For a Desktop app client Google treats the "secret" as not secret (it ships inside the program);
PKCE protects the sign-in.

## Builds for other people

People who install Kakangkung have no `google-client.json`: the client must be built into
`kakangkung.exe`.

- **Release workflow:** add the repository secrets `KAKANGKUNG_GOOGLE_CLIENT_ID` and
  `KAKANGKUNG_GOOGLE_CLIENT_SECRET` (Settings → Secrets and variables → Actions, or
  `gh secret set KAKANGKUNG_GOOGLE_CLIENT_ID`). `release.yml` passes them to `cargo build`.
- **Local release build** from the downloaded file:

  ```powershell
  $c = (Get-Content -Raw "$env:LOCALAPPDATA\Kakangkung\google-client.json" | ConvertFrom-Json).installed
  $env:KAKANGKUNG_GOOGLE_CLIENT_ID = $c.client_id; $env:KAKANGKUNG_GOOGLE_CLIENT_SECRET = $c.client_secret
  cargo build --release
  ```

- **Check:** a build with the client shows **Sign in with Google** on `kakangkung://signin` in a
  fresh profile, without the "not set up yet" note.
- `google-client.json` is in `.gitignore`; never commit it.
- While the consent screen is in **Testing**, only the listed test users can sign in, and they see
  "Google hasn't verified this app". Publish the app before a public release.

## Using it

Profile button → **Sign in to Kakangkung…** → choose what to sync → **Sign in with Google**.
Finish on Google's page in the browser; the Account page then shows the Google account and sync
is on. On another PC, sign in with the same Google account: its data arrives within a few minutes
(or press **Sync now** on kakangkung://sync).

## Not included

- Chrome's own sync service: Google limits it to Google Chrome, so bookmarks synced in Chrome
  stay in Chrome (Kakangkung can import them: Settings → Import).
- Passwords are not synced.
