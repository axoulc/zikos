# Security — source spec

## Purpose

The trust model in one line: servers are honest-but-curious. Messages are
protected by Matrix E2EE; audio files are encrypted **on the device** before
upload; keys travel only inside the encrypted room. Zikos servers never hold
anything they could read.

Status: whole-blob AES-256-GCM implemented in `crates/zikos-crypto`. This
file remains the contract.

## Requirements

### Requirement: message confidentiality
All conversation content SHALL be encrypted end-to-end via Matrix
(Olm/Megolm as provided by matrix-sdk/vodozemac). The client SHALL refuse to
send to unencrypted rooms.

### Requirement: file encryption at rest (transit & server)
Every uploaded file SHALL be encrypted client-side with AES-256-GCM under a
per-file 256-bit key before leaving the device. The nonce SHALL be fresh per
encryption. Ciphertext integrity SHALL be verified on decrypt; tampered or
wrong-key decryptions SHALL fail closed with a typed error.

#### Scenario: honest-but-curious server
- **WHEN** the blob server operator inspects stored bytes
- **THEN** they see AES-GCM ciphertext only — no file names, no metadata,
  no conversation mapping beyond opaque session usage

### Requirement: key distribution
A file's key SHALL be distributed only inside the encrypted room, in the same
logical unit as the tree event referencing the blob. Compromise of the blob
server SHALL NOT enable decryption.

### Requirement: local data at rest
Local caches (messages, tree, keys, downloaded files) SHALL be stored
encrypted at rest (SQLCipher or an OS-keystore-wrapped keyfile). Plaintext
temporary files SHALL be wiped after use.

### Requirement: pure-Rust crypto policy
Cryptographic primitives SHALL come from audited pure-Rust implementations
(RustCrypto, vodozemac) unless a spec change approves an exception.

### Requirement: chunked streaming encryption (follow-up)
Large takes (>100 MB) SHALL be encrypted and uploaded in chunks with
resumable transfer (tus protocol), such that a failed upload resumes without
re-encrypting the whole file. Tracked as a follow-up change; whole-blob
encryption is accepted for files under 100 MB in the interim.

## Non-goals (for now)

- Forward-secrecy guarantees beyond what Matrix provides.
- Plausible deniability, metadata-obfuscation (SimpleX-style).
