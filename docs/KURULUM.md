# Nect Kurulum Kılavuzu

Bu kılavuz Nect'i Windows, macOS ve Linux'a kurmayı anlatır.
**Nect'i kurmak ve `.nct` dosyalarını çalıştırmak için Rust, Cargo, C/C++,
Visual Studio veya Xcode kurmanız gerekmez.** Hazır çalıştırılabilir dosya
indirilir; kaynak kod kullanıcı bilgisayarında derlenmez. Kurulum betiği
kullanıcı `PATH`'ini ayarlar; ardından yeni bir terminal açın.

> Tek komutluk kurulum, GitHub Releases'a hazır dosyalar yayınlandıktan sonra
> kullanılabilir. Yalnızca kaynak kod arşivi içeren bir sürüm yeterli değildir.

## İçindekiler

- [Ne kuruluyor?](#ne-kuruluyor)
- [Hızlı kurulum](#hızlı-kurulum)
  - [Windows](#windows)
  - [macOS](#macos)
  - [Linux](#linux)
- [Kurulum betiğinin seçenekleri](#kurulum-betiğinin-seçenekleri)
- [PATH nasıl ayarlanıyor?](#path-nasıl-ayarlanıyor)
- [Manuel kurulum](#manuel-kurulum)
- [Kaynaktan kurulum](#kaynaktan-kurulum)
- [İsteğe bağlı özellikler](#isteğe-bağlı-özellikler)
- [Kurulumu doğrulama](#kurulumu-doğrulama)
- [Kaldırma](#kaldırma)
- [Sorun giderme](#sorun-giderme)

## Ne kuruluyor?

Nect iki biçimde dağıtılır. İkisi de aynı programları çalıştırır; farkı
hangi **isteğe bağlı yerleşik fonksiyonların** derleme sırasında binaryye
girmiş olduğudur.

| | **lean** (varsayılan) | **full** |
| --- | --- | --- |
| İndirilen boyut (macOS ARM64, yerel ölçüm) | 1,34 MiB | 3,20 MiB |
| Kurulu tek binary (aynı ölçüm) | 3,00 MiB | 7,11 MiB |
| Kullanıcının derlemesi / Rust kurulumu | Gerekmez | Gerekmez |
| Dilin tamamı (sözdizimi, VM, JIT, C backend) | ✅ | ✅ |
| Yerleşik fonksiyonlar | `print`, `len`, `sort`, `range`, matematik, dizi, metin, JSON, matris/ML | lean + LSP, paket yöneticisi, FFI |
| `http_get` / `http_post` (istemci) | ❌ | ❌ |
| `http_server` / `http_route` (sunucu) | ❌ | ❌ |
| `db_open` / `db_query` (SQLite) | ❌ | ❌ |
| `gui_window` / `gui_show` (yerel arayüz) | ❌ | ❌ |
| Dil sunucusu (`nect lsp`) | ❌ | ✅ |
| Paket yöneticisi (`nect pkg`) | ❌ | ✅ |
| FFI (`extern` bildirimleri) | ❌ | ✅ |

Boyutlar platforma ve sürüme göre değişir; bunlar macOS ARM64 release
profilinin yerel ölçümleridir, tüm platformlar için garanti değildir.

**Hangisini kurmalıyım?** Çoğu kullanıcı için **lean** yeterlidir:
dilin çekirdeği içindedir ve ihtiyaç duyulmayan bir GUI
araç kütüphanesi ya da veritabanı motoru indirmez. Dil sunucusu, paket
yöneticisi ve FFI'yi de istiyorsanız **full** kurun.

lean binary'de `gui_window` gibi bir isteğe bağlı fonksiyonu çağırmak sessizce
başarısız olmaz; hangi özelliğin gerektiğini söyler:

```console
$ nect run demo.nct
error: 'gui_window' needs the `gui` feature (rebuild with --features gui)
```

## Hızlı kurulum

### Windows

PowerShell'i açın ve tek satırı yapıştırın:

```powershell
irm https://github.com/necatiprok1/nect/releases/latest/download/install.ps1 | iex
```

`full` sürümü için:

```powershell
irm https://github.com/necatiprok1/nect/releases/latest/download/install.ps1 -OutFile install.ps1
# Gerekirse: Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\install.ps1 -Full
```

İndirilen betiği **önce okumak** isterseniz PowerShell yürütme ilkesi geçici
olarak gevşetilir (bu değişiklik kalıcı değildir, yalnızca bu oturum içindir):

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
irm https://github.com/necatiprok1/nect/releases/latest/download/install.ps1 -OutFile install.ps1
notepad install.ps1
.\install.ps1
```

Kurulum yeri `%LOCALAPPDATA%\Nect\bin` olur ve kullanıcı `PATH`'ine eklenir.
Yönetici yetkisi gerekmez, çünkü sistem `PATH`'i değil kullanıcı `PATH`'i
değiştirilir. Yeni bir terminal açtığınızda `nect` çalışır.

### macOS

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh
```

`full` sürümü için:

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh -s -- --full
```

> Yukarıdaki `--full` biçimi bu depodaki `scripts/install.sh` sürümüyle
> uyumludur; yayınlanan betik `NECT_FULL=1` de kabul eder:
>
> ```sh
> curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | NECT_FULL=1 sh
> ```

Kurulum yeri `~/.local/bin` olur ve `~/.zshrc` (veya `~/.bashrc`) dosyasına
`PATH` satırı eklenir. Değişikliğin geçerli olması için ya yeni bir terminal
açın ya da şu komutu çalıştırın:

```sh
source ~/.zshrc
```

### Linux

macOS ile aynı komut geçerlidir:

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh
```

Kurulum yeri varsayılan olarak `~/.local/bin`'dir, yani `sudo` gerekmez.
Sistemin tamamı için `/usr/local/bin` istiyorsanız:

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | INSTALL_DIR=/usr/local/bin sh
```

> `/usr/local/bin` yazılabilir değilse `sudo` ile yeniden deneyin:
> `curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sudo sh`

Debian/Ubuntu'da `curl` ve `tar` kurulu değilse:

```sh
sudo apt install curl tar
```

## Kurulum betiğinin seçenekleri

Ortam değişkeni olarak verilir, böylece boruya (pipe) ile de çalışırlar.

| Değişken | Varsayılan | Açıklama |
| --- | --- | --- |
| `INSTALL_DIR` | `~/.local/bin` (Linux/macOS) | Binary'nin konacağı dizin |
| `NECT_VERSION` | son sürüm (boş) | Sabitlenecek sürüm etiketi, ör. `v0.1.0` veya `0.1.0` |
| `NECT_FULL` | `0` | `1` ise `full` sürümü kurulur |
| `NECT_NO_PATH` | `0` | `1` ise `PATH` hiç değiştirilmez |

PowerShell tarafındaki karşılıkları:

| Parametre | Açıklama |
| --- | --- |
| `-InstallDir <yol>` | Binary'nin konacağı dizin |
| `-Version <etiket>` | Sabitlenecek sürüm |
| `-Full` | `full` sürümü |
| `-NoPath` | `PATH` değiştirilmez |

Betikler indirilen dosyanın SHA-256 özetini `SHA256SUMS` ile karşılaştırır.
Özet tutmuyorsa, liste indirilemiyorsa veya arşiv listede yoksa kurulum
**durdurulur**; mevcut `nect` dosyasına dokunulmaz.

## PATH nasıl ayarlanıyor?

Amaç, kurulumdan sonra hiçbir şeyi elle düzenlemeden `nect` yazabilmenizdir.

- **Windows:** kullanıcı `PATH` ortam değişkenine eklenir
  (`[Environment]::SetEnvironmentVariable(..., 'User')`). Yönetici yetkisi
  gerekmez, sistem değiştirilmez. Değişiklik yalnızca yeni terminallerde
  görünür.
- **macOS / Linux:** kullandığınız kabuğun profil dosyasına (`~/.zshrc`,
  `~/.bashrc`, `fish` için `~/.config/fish/config.fish`) `# added by the Nect
  installer` işaretli bir satır eklenir.

Betikler bu konuda bilinçli olarak temkinlidir:

- Dizin zaten `PATH` içindeyse hiçbir şey değiştirilmez.
- Daha önce eklenmişse (aynı işaret varsa) ikinci kez eklenmez; betiği tekrar
  çalıştırmak profil dosyasını şişirmez.
- Bilinen kabuğun profil dosyası yoksa oluşturulur; ilk kez kurulan bir
  hesapta da PATH ayarlanır. `sh` için `~/.profile` kullanılır.
- Desteklenmeyen bir kabukta PATH'i elle ayarlama bilgisi gösterilir.

## Manuel kurulum

Betik kullanmak istemiyorsanız:

1. [GitHub Releases](https://github.com/necatiprok1/nect/releases/latest)
   sayfasından işletim sisteminize ait arşivi indirin.
2. Arşivi açın.
3. Binary'yi bir yere kopyalayın ve çalıştırılabilir yapın.
4. O dizini `PATH`'e ekleyin.

```sh
tar -xzf nect-aarch64-apple-darwin.tar.gz
sudo install -m 755 nect-aarch64-apple-darwin/nect /usr/local/bin/nect
```

Windows'ta ZIP'i açabilir veya yayın eklerinden doğrudan **`nect.exe`**
indirebilirsiniz. Bu tek dosyayı bir klasöre koyup o klasörü kullanıcı
`PATH`'ine ekleyin. Bu bir setup sihirbazı değildir: taşınabilir çalıştırılabilir
dosyadır. Yayın derlemesi C çalışma zamanını statik bağlar; ayrı Visual C++
Redistributable kurulumu gerektirmez. `nect-full.exe` araçları da içeren seçenektir.
Linux arşivleri glibc tabanlı sistemler içindir; Alpine/musl için hazır paket
henüz yayınlanmaz.

Arşiv adında sürüm numarası yoktur (`nect-aarch64-apple-darwin.tar.gz`), çünkü
`releases/latest/download/...` adresi her sürümde aynıdır. Sürüm, arşivin
içindeki `VERSION` dosyasındadır.

İndirdiğiniz dosyayı doğrulamak için:

```sh
shasum -a 256 nect-aarch64-apple-darwin.tar.gz   # macOS
sha256sum nect-aarch64-apple-darwin.tar.gz       # Linux
```

Sonucu, indirdiğiniz sürümün `SHA256SUMS` dosyasındaki satırla karşılaştırın.

## Kaynaktan kurulum

Bu bölüm **Nect'i geliştirenler veya özel özelliklerle derleyenler** içindir;
normal kullanıcıların bu adımları izlemesine gerek yoktur. Güncel kararlı Rust
ve platformun derleme/link araçları gerekir. `Cargo.toml` içindeki
`rust-version` bilgisine ek olarak bağımlılıkların daha yeni Rust gereksinimleri
olabilir.

```sh
git clone https://github.com/necatiprok1/nect.git
cd nect
cargo install --path .
```

Belirli bir özellik kümesiyle:

```sh
cargo install --path . --features full
cargo install --path . --features gui,db
```

Yerelde denemek için kurulum yerine doğrudan çalıştırın:

```sh
cargo run -- run merhaba.nct
cargo run --all-features -- run merhaba.nct
```

`cargo build` ilk seferinde Cranelift derleyicisini de derleyeceği için birkaç
dakika sürebilir. Yalnızca ikiliyi değil, geliştirme derlemesini de sildikten
sonra disk kullanımını görmek için `cargo clean` çalıştırın.

## İsteğe bağlı özellikler

Özellikler Cargo özellikleridir ve ikiliyi **derleme** sırasında belirler.
`nect` kurulu ikilinin özelliklerini değiştiremez.

| Özellik | Sağladığı yerleşik fonksiyonlar / komutlar | Ağırlığı |
| --- | --- | --- |
| `net` | `http_get`, `http_post`, `http_request` | ~106 crate |
| `server` | `http_server`, `http_respond`, `http_listen`, `http_route`, `http_middleware`, `http_router` | ~49 crate |
| `db` | `db_open`, `db_close`, `db_exec`, `db_query`, `db_query_row`, `db_transaction`, `db_last_insert_rowid`, `db_changes` | ~9 crate + C derlemesi |
| `gui` | `gui_window`, `gui_button`, `gui_label`, `gui_text_input`, `gui_checkbox`, `gui_slider`, `gui_vstack`, `gui_hstack`, `gui_show`, `gui_poll_events`, `gui_close` | ~150 crate |
| `lsp` | `nect lsp` komutu (editör dil sunucusu) | ~52 crate |
| `pkg` | `nect pkg` komutu (paket yöneticisi) | ~60 crate |
| `ffi` | `extern` bildirimleri (native kütüphane çağrısı) | birkaç crate |
| `full` | `lsp` + `pkg` + `ffi` | — |

`gui`, `net`, `server` ve `db` özellikleri `full` grubunda **yoktur**: bunlar
büyük ve herkesin ihtiyacı değil. Onları isterseniz açıkça yazın:

```sh
cargo install --path . --features full,gui,net
```

Bir özelliğin ikilide olup olmadığını görmek için:

```sh
nect --version
```

çıktısındaki `built-ins:` satırına bakın.

## Kurulumu doğrulama

```sh
nect --version
```

```text
nect 0.1.0 (rust 1.85)
built-ins: core only
```

İlk kez kurduğunuzda `nect doctor` ile ortamı denetleyin (C derleyicisi gerekip
gerekmediğini gösterir):

```sh
nect doctor
```

Ve dilin çalıştığını görmek için:

```sh
printf 'print("Merhaba, " + "Nect!")\n' > merhaba.nct
nect run merhaba.nct
```

```text
Merhaba, Nect!
```

## Kaldırma

İkiliyi ve `PATH` girdisini kaldırmak için:

**macOS / Linux**

```sh
rm -f ~/.local/bin/nect
# profil dosyasındaki "# added by the Nect installer" bloğunu elle silin
```

**Windows**

```powershell
Remove-Item "$env:LOCALAPPDATA\Nect\bin\nect.exe"
# Kullanıcı PATH'inden dizini kaldırmak için:
[Environment]::SetEnvironmentVariable('Path', (
    [Environment]::GetEnvironmentVariable('Path','User') -split ';' |
        Where-Object { $_ -notlike "*Nect\bin" }
) -join ';', 'User')
```

## Yayınlama (Nect geliştiricileri için)

Hazır paketleri kullanıcının bilgisayarı değil **GitHub Actions** derler.
`.github/workflows/release.yml` şu platformlarda lean/full paketlerini üretir:
Windows x64, macOS ARM64/Intel ve Linux ARM64/x64.

1. Değişiklikleri GitHub'a gönderin. Bu adım yerel değişiklikleri kendiliğinden
   yayınlamaz; depoya gönderilmiş olmaları gerekir.
2. **Actions → Release → Run workflow** ile `dry-run` açıkken deneyin.
   Platform derlemeleri, arşivden çalıştırma testleri ve doğrulama başarılı
   olmalıdır. Sonuçtaki `release-assets` paketi kurulum dosyalarını da içerir.
3. Doğruladığınız commit için bir sürüm etiketi ve GitHub Release yayınlayın.
   Yayın akışı aynı etiketli kaynaktan paketleri üretip sürüme ekler.
   Mevcut bir sürüme yeniden yüklemek için workflow'u `tag` alanı doldurulmuş
   ve `dry-run` kapalı olarak çalıştırabilirsiniz.
4. Yayın eklerinde platform arşivleri, `nect.exe`, `nect-full.exe`,
   `install.sh`, `install.ps1` ve `SHA256SUMS` bulunduğunu kontrol edin.
   Ardından temiz bir makinede hızlı kurulum komutunu deneyin.

Yerel kurulum testleri Rust gerektirmez:

```sh
python3 scripts/test_install.py
```

Windows paketleri henüz imzalanmış setup/MSI değildir. İmzasız çalıştırılabilir
dosya veya indirilen PowerShell betiği işletim sistemi güvenlik uyarısı
oluşturabilir; üretim dağıtımı için kod imzalama ayrı bir yayınlama adımıdır.

## Sorun giderme

Daha fazla sorun giderme için [`docs/troubleshooting.md`](troubleshooting.md) ve
[`docs/platforms.md`](platforms.md) dosyalarına bakın.

**`nect: command not found` (macOS/Linux)**
Kurulum bitmiş olsa bile yeni bir terminal açmanız gerekir, çünkü profil
dosyası her yeni kabukta okunur. Hemen etkinleştirmek için
`source ~/.zshrc` çalıştırın.

**`curl: (6) Could not resolve host`**
Ağ yok ya da vekil (proxy) ayarlanmamış. `HTTPS_PROXY` ortam değişkenini
ayarlayın veya arşivi tarayıcıdan indirip [manuel kurulum](#manuel-kurulum)
yapın.

**PowerShell'de "running scripts is disabled on this system"**
Geçici olarak şu komutla gevşetin, sonra betiği çalıştırın:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

**`error: Checksum mismatch`**
İndirilen dosya bozuk ya da değiştirilmiş. Betiği yeniden çalıştırın. Aynı hata
tekrarlanıyorsa sorunu bildirin; sürüm çıkarma sürecinde bir sorun olabilir.

**Windows'ta "The term 'nect' is not recognized"**
Kurulumdan sonra açılmış terminaller eski `PATH`'i gösterir. Yeni bir terminal
açın. Değişikliğin gerçekten yazıldığını doğrulamak için:

```powershell
[Environment]::GetEnvironmentVariable('Path','User') -split ';' | Select-String Nect
```

**`nect build` "C compiler not found" hatası veriyor**
`nect build` sistemin C derleyicisini kullanır. Linux'ta `build-essential`,
macOS'ta `xcode-select --install`, Windows'ta Visual Studio Build Tools
gerekir. Yalnızca `nect run` ve `nect check` için gerekmez.
