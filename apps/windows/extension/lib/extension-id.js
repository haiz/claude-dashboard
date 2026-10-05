// Derives a Chromium extension id from its manifest "key" (base64 SPKI DER of
// the RSA public key): SHA-256 of the DER, first 16 bytes, each nibble mapped
// 0..f -> a..p. This is the same id Chrome computes, so a fixed "key" gives the
// unpacked build a stable id (needed by the host manifest's allowed_origins).

import { createHash } from 'node:crypto';

export function extensionIdFromKey(b64Spki) {
  const der = Buffer.from(b64Spki, 'base64');
  const hash = createHash('sha256').update(der).digest();
  let id = '';
  for (let i = 0; i < 16; i++) {
    id += String.fromCharCode(97 + (hash[i] >> 4));
    id += String.fromCharCode(97 + (hash[i] & 0x0f));
  }
  return id;
}
