"""Regenerates catalog/mozilla-roots.txt from curl's Mozilla CA bundle.

Usage: python3 .github/scripts/mozilla-roots.py cacert.pem > catalog/mozilla-roots.txt
Download cacert.pem and cacert.pem.sha256 from https://curl.se/ca/ and check the checksum first.
"""

import base64
import hashlib
import re
import sys

text = open(sys.argv[1], encoding="utf-8").read()
blocks = re.findall(r"-----BEGIN CERTIFICATE-----(.*?)-----END CERTIFICATE-----", text, re.S)
fingerprints = sorted({hashlib.sha256(base64.b64decode("".join(b.split()))).hexdigest().upper() for b in blocks})
date = re.search(r"as of: (.*)", text).group(1).strip()
print("# SHA-256 fingerprints of the roots trusted by Mozilla (NSS), used by `skadi audit` as a")
print("# reference list. Source: https://curl.se/ca/cacert.pem (Certificate data from Mozilla as of")
print(f"# {date}). Regenerate with .github/scripts/mozilla-roots.py.")
print("\n".join(fingerprints))
