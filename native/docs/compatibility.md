# Matriks kompatibilitas native

| Area | Status | Perilaku |
| --- | --- | --- |
| Local repository | Ada | Open/discover, status, staged/unstaged diff |
| Initial commit | Ada | Stage/unstage tanpa HEAD lalu commit root |
| Commit index | Ada | Menjaga staged content dari terminal; tidak stage-all otomatis |
| Rename/delete/symlink | Ada | Path native, stage/unstage file; fixture diuji |
| Branch | Dasar | Create dan checkout branch lokal pada working tree bersih |
| History | Ada | 100 commit/page; detail first-parent diff |
| Conflicts / merge / rebase | Baca saja | Tampilkan konflik, blokir native writes saat operasi berjalan |
| Hooks | Belum ada adapter | Executable hooks memblokir seluruh native writes secara konservatif |
| Signing | Belum ada adapter | `commit.gpgsign=true` memblokir native writes |
| Git filters / LFS | Belum ada adapter | Konfigurasi `filter.*` memblokir native writes |
| Git remote / HTTPS / SSH | Belum | Tidak ada network operation maupun prompt credential |
| OAuth / keyring | Belum | Memerlukan app ID proyek dan implementasi store Linux |
| Bare repository | Belum | Open ditolak dengan pesan khusus |
| Worktree | Dasar | Discover memakai path Git worktree; cek hooks di common dir |
| Submodule | Baca saja | Preview diberi pesan; staging direktori ditolak |
| Binary / file besar | Ringkas | Pesan binary atau preview dibatasi; tidak mengalokasikan seluruh patch ke UI |
| Preferences | Ada | JSON schema v1, XDG sendiri, atomic save, backup, corrupt-file protection |
| IndexedDB import | Belum | Data Electron tidak dibaca/ditimpa |
| Watcher / syntax highlight | Belum | Refresh manual; diff polos tervirtualisasi |
| X11 / Wayland | Build | Tetap membutuhkan smoke test pada masing-masing session/driver |

Semua operasi backend memakai libgit2 pada worker tunggal. Ini menserialisasi
mutasi internal lintas repository, termasuk common directory worktree. Belum ada
worker remote; pisahkan ketika fitur jaringan ditambahkan agar fetch panjang
tidak menghalangi pembacaan lokal. Lock eksternal tetap dihormati, bukan dihapus.
