# Sign in with Google (sync through Google Drive)

Kakangkung signs in with a Google account and keeps sync data (bookmarks, history, open tabs,
settings, extension list) in that account's Google Drive, in the hidden **app data folder**:
only Kakangkung can see it, and Kakangkung cannot see the person's own Drive files.

## How it works

- Sign-in is OAuth 2.0 for installed apps (RFC 8252), the flow Google requires for desktop apps:
  the consent page opens in an **external browser**, Google redirects to a one-shot listener on
  `127.0.0.1`, and the code is exchanged with PKCE (S256). If Kakangkung is the Windows default,
  it launches an installed Edge, Chrome or Firefox from the standard installation directories
  instead of opening OAuth inside its embedded WebView2. If none is found, sign-in shows an
  actionable error. Nonstandard browser installations can be used by making that browser the
  Windows default.
- Scopes: `openid email profile https://www.googleapis.com/auth/drive.appdata`.
- The refresh token is stored DPAPI-encrypted (`google.token` in the settings database); only the
  same Windows user on the same PC can decrypt it. Sign out deletes the local setting and attempts
  revocation at Google; revocation is best effort when the network is unavailable.
- Sync itself is unchanged (`storage::sync` merges one file per device). For a Google account the
  sync folder is `%LOCALAPPDATA%\Kakangkung\Google Drive\account-…`, and a mirror thread keeps it
  equal to the Drive app data folder (uploads this PC's file, downloads the other PCs' files)
  every 2 minutes and right after each sync.

Code: `crates/app/src/google.rs`; DPAPI in `crates/platform/src/secret.rs`.

## Setting up the OAuth client (once, by whoever builds Kakangkung)

Each Kakangkung distribution needs its owner's registered Google OAuth client. A release can
bundle that client, or a local installation can supply the downloaded Desktop app client file.

1. Open <https://console.cloud.google.com/>, create (or pick) a project.
2. **APIs & Services → Library**: enable **Google Drive API**.
3. **APIs & Services → OAuth consent screen** (Google Auth Platform): app name "Kakangkung",
   user type **External**; under **Data access** add the scopes
   `openid`, `.../auth/userinfo.email`, `.../auth/userinfo.profile`, `.../auth/drive.appdata`.
   While the app is in **Testing**, add each Google account that will sign in under **Audience →
   Test users** (up to 100). Check Google's current publishing and verification requirements
   before public distribution; publishing alone does not guarantee approval or unrestricted access.
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
  "Google hasn't verified this app". Testing-mode refresh tokens can expire after seven days
  when Drive scopes are requested, so a successful initial sign-in does not prove unattended
  sync will keep working indefinitely. Configure production publishing before public release.

## Using it

Profile button → **Sign in to Kakangkung…** → choose what to sync → **Sign in with Google**.
Finish on Google's page in the browser; the Account page then shows the Google account and sync
is on. On another PC, sign in with the same Google account: its data arrives within a few minutes
(or press **Sync now** on kakangkung://sync). Both installations must use the same Google
Cloud OAuth application/project: Drive app data is isolated by application as well as account.
Using a different developer's OAuth application does not expose the first application's data.
The local mirror resets its access-token and transfer caches when the account, OAuth client or
stored credential changes. Drive file listings follow pagination before choosing an upload target.

## Not included

- Chrome's own sync service: Google limits it to Google Chrome, so bookmarks synced in Chrome
  stay in Chrome (Kakangkung can import them: Settings → Import).
- Passwords are not synced.

## Verification and operational limits

Local regression tests cover PKCE, callback state matching, external browser path selection,
account/credential cache separation, paginated Drive listings, and device-file filtering. These
checks do not authorize a real Google account or prove cloud delivery. Before shipping, test with
your own configured client and two Windows profiles/devices: consent, cancel, sign-out, switch
accounts, upload/download each selected data type, token refresh, and disabled sync. Verify that
the second account never receives the first account's data. OAuth client configuration and Google
consent must be supplied by the application owner/account holder; no Chrome credentials are used.

Sync is a merge, not a backup with deletion propagation: history uses the newest visits,
bookmarks are united, and newer settings win. Extension sync shares names; it does not install
extensions on another PC. Passwords, browser cookies, website sessions and Chrome cloud data are
not part of this sync. Account-named subdirectories separate local mirror data but are not an
access-control boundary for people who can read the Windows profile.

References:

- [Google OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app)
- [Google OAuth loopback redirect support](https://developers.google.com/identity/protocols/oauth2/resources/loopback-migration)
- [Google Drive application data](https://developers.google.com/workspace/drive/api/guides/appdata)
- [Drive files.list pagination](https://developers.google.com/workspace/drive/api/reference/rest/v3/files/list)
- [Google OAuth refresh token expiration](https://developers.google.com/identity/protocols/oauth2#expiration)
- [Chromium API keys and restricted sign-in](https://www.chromium.org/developers/how-tos/api-keys/)

## Google profile name and photo

Kakangkung reads the display name, stable account identifier (`sub`) and optional photo URL
from Google's authenticated UserInfo endpoint. The toolbar and Account page use a locally
cached photo when one is available, with the account initial as fallback. This display metadata
is independent of Chrome Sync data. Existing signed-in accounts refresh on startup and then
hourly, including when data sync is switched off. An unavailable optional photo does not fail
sign-in or data sync.

Profile images are fetched without OAuth headers, only from HTTPS Google profile-image hosts
(`googleusercontent.com` and `ggpht.com`, including subdomains). Downloads are capped at 1 MiB
and checked for supported raster signatures before caching beneath the local Google Drive
mirror's `avatars` directory. Account metadata updates are conditional on the original account
and encrypted credential still being current, so a delayed response cannot restore metadata
after sign-out or an account switch.

[Google OpenID Connect profile claims](https://developers.google.com/identity/openid-connect/openid-connect)
notes that a profile photo and display name are optional; an initial can still be expected when
Google does not provide a photo or its image cannot be retrieved.
