# KeePass KDBX 3.1 fixtures

The `keepass` crate writes only KDBX 4, so the KDBX 4 databases are made inside the tests. KDBX 3.1 files come from a real KeePass app and live here. `tests/keepass.rs` opens every `.kdbx` file in this folder, and skips the test while there are none.

To make them (synthetic data only, never real passwords):

1. In KeePass 2 (Windows) or KeePassXC, create a new database.
2. Master password: `correct horse battery staple 42`.
3. Settings:
   - KeePass 2: Database Settings → Security → File format **KDBX 3.1**, key derivation **AES-KDF**, 6,000 iterations.
   - KeePassXC: Database Settings → Security → Encryption Settings → Database format **KDBX 3.1**, key derivation **AES-KDF**, 6,000 rounds.
4. Add a group `Work` with one entry: title `Bank`, username `ada`, password `hunter2-synthetic`, URL `https://bank.example`.
5. Save it here as `kdbx31-aes.kdbx`.
6. Optional second file: the same, with the inner cipher set to ChaCha20 or Twofish if the app offers it, as `kdbx31-other.kdbx`.

Then run `cargo test -p pwm-import --test keepass`. The test says how many files it read.
