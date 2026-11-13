# Rencana migrasi GitHub Desktop Linux ke Rust + egui

Status: rencana implementasi; kode Rust belum dibuat atau diukur.

## 1. Tujuan dan arah migrasi

Membangun GitRust Desktop sebagai aplikasi desktop Linux native berbasis Rust,
`eframe`/`egui`, tanpa Electron, Chromium, Node.js, atau WebView pada runtime.
Prioritasnya adalah penggunaan RAM dan CPU rendah, startup cepat, UI responsif,
serta operasi Git yang aman dan benar.

Migrasi ini merupakan penulisan ulang bertahap, bukan konversi otomatis TSX ke
Rust. Implementasi Electron tetap dapat dibangun selama pengembangan native.
Gunakan perilaku, fixture, dan dokumentasi lama sebagai referensi; pindahkan
fitur per alur pengguna, bukan menerjemahkan seluruh class/store satu per satu.

Dokumen `Rencana perubahan` menjadi masukan untuk nama aplikasi, pilihan `git2`,
layout, dan aspirasi RAM <30 MB. Angka tersebut adalah target eksperimen,
bukan jaminan: renderer, driver GPU, font, libgit2, dan dataset memengaruhi RAM.
Keputusan kelayakan dibuat setelah prototipe diukur pada mesin referensi.

## 2. Temuan pada repository saat ini

| Area | Implementasi yang diperiksa | Arah pengganti |
| --- | --- | --- |
| Runtime/build | `package.json`, `app/package.json`: Electron 32.1.2, React 16, TypeScript, Webpack, Yarn | Cargo dan binary native |
| Window, menu, IPC | `app/src/main-process/`, `app/src/ui/main-process-proxy.ts` | Window eframe, menu egui, channel internal |
| UI | `app/src/ui/changes/`, `diff/`, `history/`, `branches/`, `commit-message/` | Komponen egui modular |
| State | `app/src/lib/app-state.ts`, `app/src/lib/stores/app-store.ts` | `AppState` dan command/event bertipe |
| Git | `app/src/lib/git/core.ts` memanggil Git melalui dugite | Adapter `git2` dengan batas kompatibilitas eksplisit |
| Commit | `app/src/lib/git/commit.ts` mereset staging lalu men-stage file pilihan | Kontrak staging baru yang mempertahankan index pengguna |
| Persistensi | Dexie/IndexedDB di `app/src/lib/databases/`; preferensi di localStorage | JSON berskema untuk MVP; SQLite hanya bila diperlukan |
| Credential | `app/src/lib/stores/token-store.ts` memakai keytar | Ekosistem keyring dengan backend Secret Service |
| Highlight | CodeMirror dan worker, dijelaskan di `docs/technical/syntax-highlighting.md` | syntect opsional dalam worker |
| Distribusi | `script/`, `.github/workflows/ci-linux.yml` | Pipeline Cargo dan packaging Linux native |
| Pengujian | `app/test/fixtures/`, panduan Git test di `docs/technical/` | Fixture yang relevan dipakai ulang dalam test Rust |

Repository sudah memiliki fitur luas seperti LFS, stash, rebase, konflik, PR,
dan integrasi shell. MVP tidak dianggap setara seluruh fitur aplikasi lama.

## 3. Cakupan rilis

### MVP lokal

- Menambah/membuka repository yang sudah ada, daftar repository terakhir.
- Melihat branch aktif, membuat branch, checkout dengan pemeriksaan perubahan lokal.
- Status staged, unstaged, untracked, dan konflik yang dibedakan jelas.
- Diff teks per file untuk index dan working tree, warna tambah/hapus, nomor baris.
- Stage/unstage seluruh file dan commit isi index dengan summary/description.
- Riwayat commit berhalaman dan detail commit yang dimuat saat dipilih.
- Pesan error yang dapat ditindaklanjuti serta penyimpanan preferensi dasar.

### Beta terhubung

- Clone, fetch, push, upstream, ahead/behind, serta pull fast-forward saja.
- HTTPS dengan PAT/keyring, SSH agent, kemudian OAuth device flow GitHub.com.
- Deteksi divergensi/konflik dengan penjelasan; jangan otomatis reset atau rebase.
- Import daftar repository dan preferensi terpilih dari aplikasi lama.

### Setelah beta

Stage per hunk/baris, merge dan resolusi konflik, stash, rebase, cherry-pick,
revert/amend, Git LFS, signing, submodule lanjutan, PR/checks, Enterprise,
proxy khusus, image diff, serta updater otomatis diprioritaskan setelah core stabil.
Untuk MVP, kondisi yang belum didukung harus terdeteksi dan diberi status jelas;
operasi berisiko diblokir, bukan diam-diam mengubah perilaku repository.

## 4. Arsitektur target

Mulai dengan satu Cargo package di `native/` agar struktur sederhana dan build
Electron tetap utuh. Pisahkan modul melalui interface; pecah menjadi workspace
multi-crate hanya bila ada kebutuhan reuse atau waktu kompilasi yang nyata.

```text
native/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  src/
    main.rs                 # konfigurasi dan eframe::run_native
    lib.rs                  # modul yang dapat diuji tanpa window
    app.rs                  # implementasi eframe::App dan komposisi layout
    state.rs                # AppState, selection, loading/error, operation ID
    command.rs              # permintaan UI dan hasil backend
    error.rs                # error domain bertipe
    worker.rs               # antrean bounded, eksekusi, cancel, shutdown
    git/
      mod.rs                # interface GitBackend dan adapter git2
      status.rs
      diff.rs
      index.rs
      commit.rs
      branch.rs
      history.rs
      remote.rs
      compatibility.rs      # deteksi fitur yang memerlukan adapter tambahan
    auth/
      mod.rs
      credentials.rs
      device_flow.rs
    storage/
      mod.rs                # JSON berskema dan penulisan atomik
      migration.rs
    ui/
      mod.rs
      components/
        repository_list.rs
        branch_switcher.rs
        file_list.rs
        diff_view.rs
        commit_form.rs
        history_view.rs
    platform/
      mod.rs                # XDG, dialog, browser/editor, clipboard
  tests/
    git_workflows.rs
    compatibility.rs
    storage_migration.rs
  benches/
  packaging/
```

Aliran data: aksi UI → `Command` → worker → `Event` → pembaruan `AppState`
→ repaint. UI hanya membaca snapshot/model miliknya dan mengirim intent.
Backend tidak bergantung pada egui; perubahan tema/layout tidak menyentuh Git.

### Threading dan pembaruan state

1. Semua I/O Git, pembacaan file, storage, keyring, jaringan, dan highlighting
   berjalan di luar thread UI, termasuk status dan diff lokal.
2. Mulai dengan worker berbasis `std::thread` dan channel bounded. Batasi jumlah
   worker, antrean, payload, dan cache; hindari thread baru untuk setiap klik.
   UI memakai pengiriman non-blocking dan menggabungkan refresh duplikat.
3. Pisahkan pekerjaan jaringan panjang dari read lokal agar fetch tidak membuat
   seluruh UI menunggu. Serialisasi mutasi per repository/common Git directory,
   termasuk worktree yang berbagi refs; refresh setelah mutasi selesai.
4. Handle `git2::Repository` dan objek pinjamannya dimiliki worker. Kirim model
   hasil yang owned; jangan mengirim referensi ke buffer libgit2 lintas thread.
5. Sertakan repo ID, generation, dan request ID. Buang hasil usang setelah user
   berpindah repository/file; progress digabung agar tidak memenuhi antrean.
6. Cancellation bersifat kooperatif, menggunakan callback bila tersedia.
   Jangan mengklaim operasi mutasi sudah dibatalkan bila commit/ref sudah berubah;
   lakukan refresh dan tampilkan hasil aktual. Shutdown tidak menunggu di UI loop.
7. Tambahkan runtime Tokio hanya jika integrasi HTTP/async membutuhkannya.
   Fungsi libgit2 yang blocking tetap di worker/`spawn_blocking`, bukan executor UI.

### Perilaku GUI

- Sidebar: repository aktif, branch, daftar staged/unstaged; commit form di bawah.
- Panel utama: Changes/History, diff file terpilih, status operasi dan toolbar remote.
- Virtualisasi file list, history, dan diff; jangan membuat widget untuk semua baris.
- Repaint karena input/event/progress dengan `request_repaint`; hindari loop 60 FPS
  permanen saat idle. Target 60 FPS berlaku ketika ada interaksi.
- Sediakan keyboard navigation, fokus, copy/paste, scaling/HiDPI, tema gelap/terang,
  serta uji input method dan aksesibilitas dalam evaluasi prototipe.

## 5. Pilihan dependensi dan batas kompatibilitas

| Kebutuhan | Pilihan awal | Ketentuan |
| --- | --- | --- |
| GUI | `eframe`/`egui` | Aktifkan hanya renderer dan integrasi platform yang dipakai |
| Git | `git2`/libgit2 | Uji semantik terhadap fixture dan Git CLI |
| Error | `thiserror`; `anyhow` pada bootstrap bila perlu | Error domain untuk recovery UI |
| Preferensi | `serde`, `serde_json` | Versi schema, atomic replace, tanpa token |
| Highlight | `syntect`, opsional | Lazy load, subset syntax, batas ukuran/waktu |
| Credential | `keyring-core` dan store Secret Service yang sesuai | Audit API/fitur; jangan menyalin konfigurasi keyring versi lama |
| HTTP | Satu client HTTP dengan timeout | Pilih setelah spike auth; hindari dua stack TLS tanpa kebutuhan |
| Watcher | `notify`, setelah baseline | Debounce, fallback refresh manual dan saat window fokus |
| Log | `tracing` dan subscriber minimal | Rotasi/batas ukuran; redaksi credential |

Referensi versi yang teramati saat menyusun rencana: eframe 0.37.0 dan git2
0.21.0. Ini kandidat evaluasi, bukan pasangan yang sudah diuji build. Pada fase
prototipe, tetapkan MSRV/toolchain, fitur, versi dependency, serta `Cargo.lock`
setelah build dan smoke test berhasil. Gunakan `eframe::egui` bila cukup untuk
mencegah versi egui ganda. Ekosistem keyring versi 4 memisahkan core/store;
pilih backend Linux eksplisit agar tidak membawa semua store platform.

`git2` tidak boleh diasumsikan identik dengan Git CLI untuk hooks, signing,
credential helper, SSH config, filter/LFS, dan konfigurasi kompleks. Buat matriks
dukungan sejak awal. Jika kompatibilitas memerlukan Git CLI, isolasikan adapter
opsional dengan executable/argumen terstruktur, tanpa shell interpolation, dan
laporkan dependensi tersebut. Runtime tetap native; cakupan tanpa Git eksternal
harus dijelaskan per fitur. Jangan menulis ulang protokol LFS demi menghindari CLI.

## 6. Semantik Git yang harus dijaga

- Bedakan HEAD→index dan index→working tree. File yang berubah di keduanya
  muncul pada dua kelompok dengan diff sesuai sumbernya.
- Commit menggunakan isi index yang terlihat oleh pengguna. Jangan menyalin
  perilaku `unstageAll` dari implementasi lama tanpa keputusan UX eksplisit.
  Pertahankan staged changes dari terminal; summary wajib, identity diperiksa,
  initial commit dan detached HEAD ditangani terpisah.
- Setelah checkout/stage/commit, validasi ulang state repository dan refresh.
  Jangan overwrite perubahan lokal, memaksa checkout, force-push, atau hard reset.
- Tangani unborn branch, rename/delete, symlink, executable bit, CRLF, binary,
  nama file non-UTF-8, worktree, dan external index lock secara eksplisit.
- Pull MVP hanya fast-forward setelah pemeriksaan state/upstream. Branch divergen,
  konflik, dan operasi merge/rebase yang sedang berlangsung mendapat pesan khusus.
- Repository dengan hooks/signing wajib tidak boleh diam-diam membuat commit
  melalui jalur yang melewati kebijakan itu. Gunakan jalur kompatibel atau blokir.

## 7. Strategi RAM, CPU, dan diff

- Muat status ringkas lebih dahulu; diff hanya untuk file aktif, history berhalaman.
- Cache berdasarkan repo, HEAD/index/worktree generation, file, dan opsi diff;
  invalidasi setelah event eksternal atau mutasi, dengan LRU berbatas byte.
- Gunakan satu buffer owned seperti `Arc<[u8]>` ditambah rentang byte per baris.
  Borrow `&[u8]` selama parsing lokal; salin sekali bila buffer callback tidak
  bertahan. Hindari `String` dan clone seluruh diff per baris/per frame.
- Pertahankan path native untuk operasi; konversi lossy hanya untuk tampilan.
- Batasi preview awal, misalnya 2 MiB/file dan 20.000 baris; tampilkan pemberitahuan
  serta aksi membuka di editor. Batas ini perlu divalidasi, bukan dianggap optimal.
  Terapkan pemeriksaan sebelum membangun patch besar bila memungkinkan.
- Highlight berjalan setelah diff polos tampil. Jaga state lexer sisi lama/baru
  dan konteks multiline; hanya tokenize hunk terpisah akan menghasilkan warna salah.
- Batasi grammar, font atlas, texture, thumbnail, antrean history, dan cache libgit2
  yang tersedia. Lepaskan data repository lama saat berpindah.
- Watcher hanya memberi invalidasi, dengan debounce awal 250–500 ms. Hindari
  scan rekursif/polling seluruh repo terus-menerus; sediakan refresh manual.
- Bandingkan renderer `glow` dan `wgpu` pada prototipe. Pilih berdasarkan RAM,
  CPU, startup, dan kompatibilitas Wayland/X11, bukan asumsi ukuran binary saja.
- Evaluasi release profile (`lto`, `codegen-units`, strip) setelah baseline.
  Ukuran binary, peak RAM, dan waktu startup adalah metrik berbeda.

## 8. Credential dan migrasi data

Gunakan namespace aplikasi sendiri pada XDG config/data/cache dan keyring;
jangan menulis ke profil Electron. Preferensi JSON memiliki schema version,
default aman, backup, dan atomic replace. Simpan path repository, UI preference,
serta identitas akun non-rahasia saja; jangan menyimpan token di JSON/log/URL.

Mulai dengan PAT dan SSH agent. OAuth device flow membutuhkan OAuth app milik
proyek dengan device flow aktif; jangan memakai client secret aplikasi lama.
Polling mengikuti interval server, termasuk slow_down, expiry, penolakan,
dan pembatalan. Pisahkan credential berdasarkan host/akun dan batasi pengiriman
token ke host yang sesuai. Pertahankan validasi TLS dan SSH host key.

Keyring terkunci/tidak tersedia harus memberi recovery yang jelas; gunakan token
di memori untuk sesi bila dipilih pengguna, tanpa fallback plaintext otomatis.
Credential Git HTTPS dan login API perlu dipetakan eksplisit; token API tidak
otomatis menyelesaikan semua konfigurasi SSH/helper.

Import data dilakukan melalui export JSON berskema dari aplikasi lama atau input
daftar path; jangan mengasumsikan Rust dapat membaca IndexedDB langsung. Preview
data sebelum import, deduplikasi path, toleransi repo hilang, dan jangan import
token diam-diam. Pengguna dapat login kembali pada native.

## 9. Tahapan implementasi dan kriteria selesai

### Fase 0 — Audit dan baseline

- [ ] Inventaris alur UI, operasi Git, platform integration, dan fitur kompatibilitas.
- [ ] Rekam baseline Electron pada mesin referensi dan fixture kecil/sedang/besar.
- [ ] Tetapkan kontrak staging, dukungan hooks/signing/LFS, dan prioritas fitur.

Selesai bila matriks MVP/beta/ditunda dan hasil baseline tersimpan di repository.

### Fase 1 — Prototipe kelayakan native

- [ ] Buat `native/`, Cargo manifest/lockfile/toolchain, window dan panel dasar.
- [ ] Tambahkan worker, command/event, generation ID, error/loading/cancel.
- [ ] Bandingkan renderer; uji RAM idle, startup, Wayland/X11, input dan HiDPI.
- [ ] Tentukan dependency/features minimum dan target memori yang terbukti layak.

Selesai bila window responsif selama simulasi pekerjaan panjang; hasil ukur
menentukan apakah target <30 MB dapat dipertahankan atau perlu direvisi.

### Fase 2 — Git baca-saja dan diff

- [ ] Open repository, status staged/unstaged/untracked, branch dan HEAD.
- [ ] Diff per file, virtualisasi, batas preview, binary dan error state.
- [ ] History berhalaman dan detail commit on-demand.
- [ ] Port fixture relevan, termasuk path/rename/CRLF/initial commit/worktree.

Selesai bila status dan diff sesuai oracle Git pada fixture, tanpa mutasi repository.

### Fase 3 — MVP lokal yang dapat menulis

- [ ] Stage/unstage file, commit index, identity dan validasi message.
- [ ] Create/checkout branch dengan proteksi perubahan lokal dan lock eksternal.
- [ ] Deteksi hooks/signing/fitur unsupported serta recovery sesudah gagal.
- [ ] Persistensi preferensi dan daftar repository secara atomik.

Selesai bila alur edit→stage→commit→history dan checkout lulus test integrasi,
termasuk staged changes dari terminal yang tetap dipertahankan.

### Fase 4 — Beta remote dan autentikasi

- [ ] Clone/fetch/push/pull fast-forward, progress dan cancellation.
- [ ] PAT/keyring, SSH agent/known hosts, upstream dan ahead/behind.
- [ ] OAuth device flow memakai app proyek; uji expired/denied/slow_down/offline.
- [ ] Tampilkan divergensi, auth failure, dan konfigurasi yang belum didukung.

Selesai bila remote lokal terkontrol dan uji manual GitHub berhasil; test CI
default tidak membutuhkan credential pribadi atau layanan eksternal.

### Fase 5 — Optimasi berdasarkan profil

- [ ] Ukur heap, RSS/PSS, peak operasi, frame time, I/O, startup, dan idle CPU.
- [ ] Tambahkan highlight opsional, watcher debounce, cache dengan byte budget.
- [ ] Perbaiki bottleneck terbesar; ulangi benchmark hanya pada perubahan relevan.
- [ ] Jalankan uji berpindah repo/file berulang untuk mendeteksi pertumbuhan RAM.

Selesai bila target kinerja terpenuhi atau selisih dan keputusan revisinya tercatat.

### Fase 6 — Packaging, migrasi, dan cutover

- [ ] CI Cargo: fmt, clippy, test, release build dan audit dependency/license.
- [ ] Panduan build Ubuntu/Arch sesuai dependency native dan renderer terpilih.
- [ ] Packaging `.deb`, `.rpm`, AppImage, desktop entry, ikon, serta checksum.
- [ ] Smoke test paket pada X11/Wayland; mulai x86_64, lanjut ARM64 setelah stabil.
- [ ] Import data terpilih, backup, dan petunjuk kembali memakai aplikasi lama.
- [ ] Rilis native side-by-side dengan app ID/profile berbeda, lalu evaluasi beta.

Selesai bila paket dapat dipasang, dijalankan, dan dihapus bersih; data Git dan
profil lama tetap dapat dipakai untuk kembali. Penghapusan Electron baru dilakukan
setelah cakupan rilis native disepakati dan gate beta terpenuhi, pada perubahan terpisah.

Urutan dependensi: 0 → 1 → 2 → 3 → 4 → 5 → 6. Instrumentasi kinerja dan test
ditambahkan sejak fase pertama, bukan ditunda sampai akhir. Timeline ditetapkan
setelah prototipe dan audit kompatibilitas; belum ada estimasi durasi terverifikasi.

## 10. Pengukuran dan target penerimaan

Semua angka berikut adalah sasaran awal, belum hasil benchmark.

| Metrik | Sasaran awal | Metode |
| --- | --- | --- |
| RAM idle | Aspirasi RSS <30 MiB pada repo kecil | Ukur saat window stabil, setelah 30 detik idle |
| RAM relatif | PSS turun ≥50% dibanding Electron pada skenario sama | Jumlah seluruh process tree kedua aplikasi |
| Startup | Warm start p95 ≤500 ms sampai frame pertama yang interaktif | Instrumentasi monotonic, ≥20 run; cold start dilaporkan terpisah |
| Respons UI | Frame time p95 ≤16,7 ms saat scroll/input biasa | Instrumentasi release build, refresh rate tetap |
| CPU idle | Rata-rata <1% satu core selama 60 detik tanpa perubahan | Tanpa animasi, refresh atau polling aktif yang tidak perlu |
| Cache aplikasi | Budget awal 16 MiB untuk diff/highlight/history | Cache dihitung dalam target RAM, diturunkan bila budget tidak cukup |
| Stabilitas | Tidak ada pertumbuhan RAM monoton setelah siklus berulang | Buka/tutup dan pindah repo/file ≥100 kali, amati plateau |

Catat CPU/GPU, driver, distro, session Wayland/X11, resolusi/scaling, Git,
Rust/toolchain, renderer, build profile, dan revision. Gunakan dataset deterministik:
repo kecil, repo ±10.000 file/50.000 commit, diff besar, binary besar, serta banyak
perubahan; simpan karakteristik aktualnya bersama hasil. Jangan mengukur hanya repo
kosong atau membandingkan debug Rust dengan release Electron.

Pisahkan RSS/PSS idle dan peak operasi dari VRAM/driver; gunakan `/proc/*/smaps_rollup`,
`pidstat`, `/usr/bin/time -v`, dan profiler heap/CPU sesuai kebutuhan. Startup
process-launch hingga frame pertama berbeda dari waktu status repository siap;
laporkan keduanya. Durasi jaringan dilaporkan terpisah dari respons UI.

## 11. Strategi verifikasi

- Unit test: reducer/event, stale result, error mapping, range diff, pagination,
  serialization dan schema migration.
- Integration test: repository sementara dan remote bare lokal; cocokkan hasil
  Git via status porcelain, index/tree, refs, parent commit, dan isi file.
- Pertahankan fixture lama tanpa menimpa aslinya. Siapkan ulang direktori `_git`
  fixture menjadi `.git` hanya di direktori sementara bila diperlukan.
- Uji kegagalan: index lock, repo hilang, permission denied, disk penuh, HEAD
  berubah dari terminal, auth expired, network timeout dan cancellation.
- Uji keselamatan mutasi: gagal commit tidak menghapus staging; checkout tidak
  membuang edit; fetch/push gagal tidak mengklaim sukses; operasi unsupported jelas.
- UI smoke/manual: keyboard, scaling, multiline commit, scroll diff besar,
  pergantian repo saat worker aktif, keyring terkunci dan sesi X11/Wayland.
- Setelah Cargo package tersedia, gate otomatis: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, dan
  `cargo build --release --locked`, dijalankan dari `native/` dengan fitur rilis.

## 12. Risiko utama dan mitigasi

| Risiko | Mitigasi dan titik keputusan |
| --- | --- |
| Target <30 MiB tidak realistis pada driver tertentu | Ukur fase 1; laporkan overhead dan revisi budget secara eksplisit |
| Perbedaan Git CLI dengan libgit2 | Compatibility matrix, oracle test, adapter opsional atau blokir fitur |
| Scope setara GitHub Desktop terlalu besar | Rilis MVP lokal dahulu, kemudian beta remote dan fitur lanjutan |
| Race dengan terminal atau worktree lain | Serialisasi internal, validasi ulang state, hormati lock Git, refresh eksternal |
| Diff/highlight menghabiskan RAM atau CPU | Lazy load, batas preview, virtualisasi dan cache byte budget |
| Import data/credential rusak | Export/import berskema, backup, profile baru, login ulang |
| Regresi desktop Linux | Uji paket pada X11/Wayland dan dependency native yang dideklarasikan |
| Nama/aset upstream dipakai tanpa audit | Pertahankan lisensi MIT/atribusi; ikuti batas trademark pada README |

## 13. Langkah implementasi pertama

Mulai dari fase 0–1: baseline aplikasi lama, prototipe window native, dan satu alur
baca-saja open repository→status→diff menggunakan worker. Deliverable pertama:
`native/` yang dapat dibangun, fixture test dasar, serta laporan perbandingan
renderer/RAM/startup. Jangan memulai dari OAuth, seluruh fitur PR, atau menghapus
dependency Electron sebelum prototipe ini membuktikan arah teknisnya.

## Referensi teknis

- [eframe: runtime native, API, dan feature flags](https://docs.rs/eframe/latest/eframe/)
- [egui: implementasi dan dokumentasi proyek](https://github.com/emilk/egui)
- [git2: binding libgit2 dan lifetime objek repository](https://docs.rs/git2/latest/git2/)
- [Keyring: pemisahan core dan credential store](https://docs.rs/keyring/latest/keyring/)
- [GitHub OAuth: device flow](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow)

Referensi lokal utama: `Rencana perubahan`, `README.md`, `app/package.json`,
`app/src/lib/git/commit.ts`, `docs/technical/oauth.md`, dan
`docs/technical/syntax-highlighting.md`. API dependency perlu diverifikasi lagi
saat implementasi; dokumen ini tidak menyatakan adanya build Rust yang sudah lulus.
