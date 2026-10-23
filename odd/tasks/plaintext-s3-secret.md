# Plaintext secret_access_key in S3Dive's rclone.conf

Bucket connections fail with `SignatureDoesNotMatch` even when the credentials are
correct, because S3Dive stores `secret_access_key` obscured and rclone's s3 backend
does not reveal obscured values.

## Specs

- S1: The user reported, for a Backblaze B2 S3 remote: `Signature mismatch: the secret
  access key does not match the access key ID. rclone said: ... api error
  SignatureDoesNotMatch: Signature validation failed` ("I'm getting this error but the
  credentials are correct"). Test connection must succeed with valid credentials.
- S2 (from L2, decision: "Texto plano en el conf 0600"): `secret_access_key` is written
  to S3Dive's own `rclone.conf` verbatim, exactly as entered. No `rclone obscure`
  round-trip for the s3 backend; the file keeps mode `0600`.
- S3: The secret never enters process arguments, `state.json`, log output, or any value
  returned to the UI.

## Tasks

| ID | Spec | Route | Commit | Status |
| --- | --- | --- | --- | --- |
| T1 | S2, S3 | core + app: write the secret verbatim, delete the obscure helpers | `719210d` | done: 57 core tests, `cargo check --all-targets`, clippy `-D warnings` |
| T2 | S1, S2 | copy + docs: UI hint, README security bullets, troubleshooting row | this commit | done: `pnpm typecheck` |
| T3 | S1, S2, S3 | verify: live B2 listing with an app-shaped config | n/a | done: obscured secret -> 403 `SignatureDoesNotMatch`, verbatim secret -> bucket listing, config byte-identical apart from the secret line |

Delivery: two reviewable slices (behavior, then copy/docs). No auto-migration is
included on purpose: repairing a value written by the previous version would need
`rclone reveal <obscured>` with the blob in the process arguments, which exposes the
secret to local users through `ps`. Existing buckets are repaired by re-entering the
secret once (Edit -> Save); the troubleshooting table now says so.

## Log

- L1: "I'm getting this error but the credentials are correct
  /home/jcampos/Pictures/Screenshot from 2026-10-23 12-41-50.png"
- L2 (decision): "Texto plano en el conf 0600 (simple)" — store the secret in plaintext
  in the 0600 app-owned config instead of obscured.
- L3 (diagnosis evidence): hand-signed SigV4 `ListBuckets` with the secret stored in
  `~/.config/dev.s3dive.app/rclone.conf` returns 200 from `s3.us-east-005.backblazeb2.com`.
  rclone v1.75.2 signs with the 63-char obscured blob instead of the decrypted secret
  (recomputed its signature from an intercepted request: it matches the blob, not the
  secret). The same secret as a plaintext config value or as `--s3-secret-access-key`
  lists buckets fine, and rclone's own `config create` also writes it plaintext. A value
  produced by `rclone config password` (rclone's own obscuring) fails the same way, so the
  fault is rclone's s3 read path, not S3Dive's obscure call. Obscured values are still
  revealed by other backends (`crypt` rejects a plaintext password).
