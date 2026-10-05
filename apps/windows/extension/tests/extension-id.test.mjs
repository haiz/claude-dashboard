import { test } from 'node:test';
import assert from 'node:assert/strict';
import { extensionIdFromKey } from '../lib/extension-id.js';

// Pins the committed manifest "key" to the id used in the host manifest's
// allowed_origins and scripts/register-dev-host.ps1. If the key changes, this
// fails and the id must be updated everywhere together.
test('the committed key yields the fixed extension id', () => {
  const key =
    'MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAsyGVIgK+AYPWrhrCoY+/' +
    'TYX0GivGfWh+uyUXLbCfHU4ZlyRrTwk/yE8SZVOCfmmyhJlGNSeXiTkv7tb6HyV' +
    '6jOHPA3t3INp1A13i5D0eRhrqFbq7EdhWK2WnkibpSR+/4tPQRGocZRNbwXMJrH' +
    'zUg+ecNJWV+M3G5JT/kGAV2v8o4taKJuZnNNxOJ1t/x9IelmS3zFQX322VPqX+d' +
    'eWj7ZQkPW5L8EbjnjEtfFIxa8JAAtyZbb55kgQLgr4d3+eXAmZiGz+AgR8ROW/B' +
    'MSDWPbNiwULofoJs0XXhhyD0Ipkf3HCUDKGg+glzMTaPt/O14ZZhMWgNlJD58Ik' +
    'xqWLj0QIDAQAB';
  assert.equal(extensionIdFromKey(key), 'cadpjcfajhlgdaipepkdojcehfmkkedh');
});
