# Publishing Claude Dashboard Sync

Everything the Chrome Web Store (and Edge Add-ons) submission asks for, and the
code change that switches the app's Install button from the unpacked fallback to
the store page.

## 1. Package

```powershell
powershell -File apps/windows/scripts/pack-extension.ps1 -Out claude-dashboard-extension-store.zip -Store
```

`-Store` drops the manifest `key`; the store assigns the item its own id.

## 2. Listing

- **Name:** Claude Dashboard Sync
- **Category:** Productivity (Tools)
- **Summary (132 chars max):** Sends your claude.ai session to the Claude Dashboard desktop app, so it can show your usage limits.
- **Description:**

  > Claude Dashboard Sync connects your browser to the Claude Dashboard desktop
  > app for Windows (https://github.com/haiz/claude-dashboard). The app shows
  > your Claude 5-hour and weekly usage for every account you use.
  >
  > The extension reads the claude.ai sign-in cookie of the browser profile it
  > is installed in, and hands it to the desktop app on the same computer
  > through Chrome's native messaging. It re-sends it when the cookie changes,
  > so the app keeps working after you sign in again.
  >
  > Requires the Claude Dashboard desktop app. Nothing is sent anywhere else.

- **Icon:** `icons/icon-128.png`. Screenshots (1280x800): the Add Account window's
  Browser extension tab and the Dashboard.
- **Single purpose:** Hand the claude.ai session of this browser profile to the
  Claude Dashboard desktop app on the same computer.

## 3. Permission justifications

| Permission | Why |
| --- | --- |
| `cookies` + host `https://claude.ai/*` | Read the claude.ai `sessionKey` cookie, and notice when it changes. No other site. |
| `nativeMessaging` | Hand the key to the local Claude Dashboard app (`com.claude_dashboard.bridge`). |
| `alarms` | Re-sync every 30 minutes so a renewed session reaches the app. |
| `storage` | Keep a random install id and the last sync status shown in the popup. |

Remote code: none. Data use: the session cookie is authentication information;
it is sent only to the local app, never to the developer or a third party, never
sold, and used only for the extension's single purpose.

## 4. Privacy policy

Host this section (for example as this file's GitHub URL) and give its link:

> **Claude Dashboard Sync privacy policy.** The extension reads one cookie, the
> claude.ai `sessionKey` of the browser profile it is installed in, and passes it
> through the browser's native messaging to the Claude Dashboard app on the same
> computer. The app stores it encrypted (Windows DPAPI) and uses it only to read
> that account's usage from claude.ai. The extension stores a random install id
> and its last status in the browser's extension storage. No data is sent to the
> developer or any third party, and nothing is collected for analytics or
> advertising. Uninstalling the extension deletes its storage; removing the
> account in the app deletes the stored key.

## 5. After approval

1. Put the store id in `app/src/ext_install.rs` (`CHROME_WEB_STORE_ID`, and
   `EDGE_ADDONS_ID` if published there too).
2. Add `chrome-extension://<id>/` to `allowed_origins` in
   `bridge/com.claude_dashboard.bridge.json` and `app/wix/com.claude_dashboard.bridge.json`
   (`cargo test` fails until both list every id).
3. Release. Users who upgrade get Install buttons that open the store page; the
   unpacked steps disappear from the wizard.

Edge can install straight from the Chrome Web Store (it offers "Allow extensions
from other stores"), so an Edge Add-ons listing is optional.
