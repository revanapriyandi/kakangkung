import http from 'node:http';
import { readFile } from 'node:fs/promises';

// Synthetic credentials only. This fixture tests the page script, not Chrome's vault.
const page = `<!doctype html><meta charset="utf-8"><title>Password regression fixture</title>
<button id="toolbar" type="button">Toolbar action</button>
<form id="login"><label>Username<input id="user" name="username" autocomplete="username"></label>
<label>Password<input id="pass" type="password" autocomplete="current-password"></label>
<button id="submit">Sign in to fixture</button></form><output id="status">ready</output>
<script>
window.ipc = { postMessage(message) {
  if (message.startsWith('kk:pw:save:')) {
    document.querySelector('#status').id = 'saved';
    document.querySelector('#saved').textContent = 'save-offer-received';
  }
}};
document.querySelector('#login').addEventListener('submit', event => {
  event.preventDefault();
  document.querySelector('#login').remove();
});
</script><script src="/passwords.js"></script>`;
const server = http.createServer(async (req, res) => {
  if (req.url === '/passwords.js') {
    res.setHeader('Content-Type', 'text/javascript');
    res.end(await readFile(new URL('../assets/passwords.js', import.meta.url)));
  } else {
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    res.end(page);
  }
});
server.listen(0, '127.0.0.1', () => console.log(`http://127.0.0.1:${server.address().port}/`));
