# GitRust Desktop — native local preview

Implementasi Rust + egui berdampingan dengan aplikasi Electron. Binary native
tidak memakai Node.js, Chromium, WebView, atau proses Git untuk operasinya.
Git CLI hanya diperlukan oleh test sebagai pembanding independen.

## Fitur yang tersedia

- Buka repository lewat path/argumen CLI dan daftar recent pada profil XDG terpisah.
- Branch, staged/unstaged/untracked/conflict status, diff teks tervirtualisasi.
- Stage/unstage file, initial commit dan commit index tanpa mereset staging terminal.
- Membuat branch dan checkout ketika repository bersih.
- History per 100 commit, detail diff terhadap parent pertama dimuat saat dipilih.
- Worker dengan antrean bounded, pembatalan baca, dan penolakan hasil request usang.
- Preview dibatasi 2 MiB/20.000 baris; binary/submodule diberi pesan khusus.
- Theme, keyboard/text input standar egui, X11 dan Wayland pada build yang sama.

Ini belum setara seluruh fitur GitHub Desktop. Remote clone/fetch/push/pull,
OAuth/keyring, signing, hooks, filter/LFS, merge/rebase, stage parsial, dan import
profil Electron belum diimplementasikan. Aplikasi memblokir native writes bila
menemukan hooks executable, commit signing, filter Git, atau operasi merge/rebase
berlangsung. Tuntaskan operasi tersebut dengan Git. Tidak ada fallback yang
diam-diam melewati hooks atau menonaktifkan signing.

## Build di Linux

Rust dipin ke 1.88.0 melalui `rust-toolchain.toml`. Rustup memasang toolchain dan
komponen yang sesuai saat perintah Cargo dijalankan. Dependency terkunci pada
`Cargo.lock`; eframe 0.33.3 dipilih karena mendukung toolchain ini. Versi kandidat
0.37 dalam rencana awal membutuhkan Rust yang lebih baru.

Ubuntu/Mint (perintah instalasi dijalankan sendiri bila dibutuhkan):

```sh
sudo apt install build-essential pkg-config cmake git libx11-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev
```

Arch Linux:

```sh
sudo pacman -S --needed base-devel pkgconf cmake git libx11 libxkbcommon wayland mesa
```

Libgit2/zlib dapat dibangun oleh dependency Cargo; tidak perlu memasang Node/Yarn.
Renderer awal adalah OpenGL (`glow`). Perintah berikut dijalankan dari direktori
`native/`:

```sh
cargo build --release --locked
cargo run --release --locked -- /path/ke/repository
```

Path dapat ditempel ke kolom repository lalu klik **Open**. Klik file untuk diff,
**Stage file**, isi summary/description, lalu **Commit staged changes**. Tombol
**Refresh** memuat ulang perubahan eksternal; watcher otomatis belum aktif.
Checkout sengaja meminta working tree bersih agar edit tidak tertimpa.

Bandingkan renderer wgpu tanpa memasukkan kedua backend dalam binary yang sama:

```sh
cargo build --release --locked --no-default-features --features wgpu
```

Build/test backend tanpa dependency GUI:

```sh
cargo test --locked --no-default-features
```

## Profil, verifikasi, dan pengukuran

Preferensi disimpan di `$XDG_CONFIG_HOME/gitrust-desktop/preferences.json` atau
`~/.config/gitrust-desktop/preferences.json`. Isinya hanya schema version dan
20 path recent; aplikasi tidak membaca atau mengubah profil Electron. Penulisan
atomik menyimpan satu `.bak`. File corrupt/schema baru mematikan persistensi sesi,
tanpa menimpa data lama. Gunakan `--no-persist` untuk pengujian terisolasi.

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
target/release/gitrust-desktop --no-persist --inspect /path/ke/repository
target/release/gitrust-desktop --no-persist --smoke-test /path/ke/repository
python3 script/measure.py target/release/gitrust-desktop /path/ke/repository
```

`--inspect` berjalan tanpa window dan hanya membaca Git. `--smoke-test` membuka
window lalu menutupnya setelah tiga detik. Startup marker pada stderr mengukur
UI update pertama dan repository siap, bukan waktu present frame GPU. Pengukuran
idle harus memakai window nyata; angka headless tidak membuktikan target <30 MiB.
Script pengukuran menutup hanya proses yang diluncurkannya dan tidak menyimpan
profil. Hasil aktual dan keterbatasan dicatat di `docs/progress.md`.

## Batas keselamatan dan kompatibilitas

- Commit menggunakan index saat snapshot direview. Jika HEAD/index berubah dari
  terminal, mutasi ditolak hingga **Refresh**. Pemeriksaan ini mengurangi race,
  tetapi tidak mengunci seluruh aktivitas aplikasi Git eksternal secara atomik.
- Nama file native non-UTF-8 tetap berupa `PathBuf` untuk operasi; tampilan lossy.
  Root repository non-UTF-8 dapat gagal diserialisasi ke recent JSON; repository
  tetap terbuka dan aplikasi menampilkan warning penyimpanan.
- Diff yang terlalu besar/binary tidak di-highlight; syntect belum ditambahkan.
  Cancel baca bersifat kooperatif dan tidak selalu menghentikan pemindaian
  internal libgit2 seketika. Mutasi yang sudah dimulai tidak dibatalkan dari UI.
- Belum tersedia prompt untuk stash, force checkout/push, hard reset, atau
  penyelesaian konflik. Aplikasi tidak menjalankan operasi destruktif tersebut.
- Dukungan worktree, symlink, rename, CRLF, dan lock perlu terus diperluas melalui
  fixture; gunakan repository salinan untuk evaluasi awal sebelum adopsi harian.

Lihat `../plan.md` untuk roadmap dan `docs/compatibility.md` untuk matriks dukungan.
