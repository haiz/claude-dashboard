// Generates an RSA keypair for the extension and prints the manifest "key"
// (base64 SPKI public key) plus the extension id it yields. Run once; paste the
// key into manifest.json's "key" and the id into the host manifest's
// allowed_origins. Writes the private key to extension-private.pem (gitignored)
// so the same id can be regenerated; nothing in the build needs the private key.
//
//   node scripts/gen-key.mjs

import { generateKeyPairSync } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { extensionIdFromKey } from '../lib/extension-id.js';

const { publicKey, privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });

const spkiDer = publicKey.export({ type: 'spki', format: 'der' });
const keyB64 = spkiDer.toString('base64');
const id = extensionIdFromKey(keyB64);

writeFileSync(
  new URL('../extension-private.pem', import.meta.url),
  privateKey.export({ type: 'pkcs8', format: 'pem' })
);

console.log('manifest "key":', keyB64);
console.log('extension id   :', id);
console.log('host allowed_origins: chrome-extension://' + id + '/');
