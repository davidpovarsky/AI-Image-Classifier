# Certificate lifecycle

The supervisor generates CA material locally with modern algorithms, non-exportable/restrictive private-key storage, a product-specific subject, bounded validity, and an explicit fingerprint. It installs only after informed consent and verifies trust before enabling capture.

Uninstall and repair remove exactly the recorded product fingerprint. They never search broadly by subject or delete unrelated certificates. Diagnostics may include the public fingerprint and validity but never the private key.
