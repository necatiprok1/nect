# Nect kurulum kılavuzu

## Desteklenen hazır paketler

[v1.0.0 yayını](https://github.com/necatiprok1/nect/releases/tag/v1.0.0), macOS
ARM64/Intel ve Linux ARM64/x64 için sekiz lean/full arşivi, kurulum betikleri ve
`SHA256SUMS` içerir. Linux paketleri glibc tabanlı sistemler içindir; Alpine/musl
paketi henüz yayınlanmadı.

**Windows binary'leri henüz yayınlanmadı.** Yayındaki `install.ps1`, eksik Windows
binary'sinin yerine geçmez. Windows SDK lisans onayı ve GitHub Actions faturalama
engeli çözülmeden hazır Windows kurulumu vaat edilmemelidir. Windows kullanıcıları
şimdilik [kaynaktan derleme](#kaynaktan-kurulum) yolunu kullanabilir.

Hazır paketle `.nct` programlarını çalıştırmak için Rust, Cargo veya C/C++
derleyicisi gerekmez. `nect build` komutu ise sistem C derleyicisini kullanır.
Yayın etiketi `v1.0.0`, kaynakta belirtilen paket sürümü ise `0.1.0`'dır;
`nect --version` çıktısını etiketle aynı sanmayın.

## Hızlı kurulum: macOS ve Linux

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh
```

`full` için:

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh -s -- --full
```

Uzak bir betiği çalıştırmadan önce incelemek isterseniz:

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh -o install.sh
less install.sh
sh install.sh
```

Varsayılan kurulum yeri `~/.local/bin`'dir; yönetici yetkisi gerekmez. Betik
indirmeyi `SHA256SUMS` ile doğrular. Liste eksikse, arşiv listede yoksa veya özet
tutmuyorsa kurulum durur. Checksum, dosyanın yayınla eşleştiğini gösterir;
yayıncının güvenilirliğini veya kodun zararsızlığını kanıtlamaz.

Kurulumdan sonra **yeni bir terminal açın**. Desteklenen kabuklarda betik profil
dosyasına PATH girdisi ekler; desteklenmeyenlerde elle ayarlama bilgisi verir.

### Seçenekler

| Ortam değişkeni | Varsayılan | İşlev |
| --- | --- | --- |
| `INSTALL_DIR` | `~/.local/bin` | Binary dizini |
| `NECT_VERSION` | Son yayın | Sabit yayın etiketi, ör. `v1.0.0` |
| `NECT_FULL` | `0` | `1` ile full derleme |
| `NECT_NO_PATH` | `0` | `1` ile PATH değişikliğini atla |

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | NECT_FULL=1 sh
```

Windows paketleri yayınlandığında PowerShell betiği `-InstallDir`, `-Version`,
`-Full` ve `-NoPath` seçenekleriyle kullanılabilir; şimdilik hazır kurulum yolu
değildir.

## Manuel kurulum

1. [Releases](https://github.com/necatiprok1/nect/releases/latest) sayfasından
   platformunuza ve lean/full seçiminize uygun arşivi ve `SHA256SUMS`'ı indirin.
2. Arşivin SHA-256 özetini listedeki değerle karşılaştırın.
3. Arşivi açın ve içindeki `nect` dosyasını PATH üzerindeki bir dizine koyun.

macOS ARM64 lean örneği:

```sh
shasum -a 256 nect-aarch64-apple-darwin.tar.gz
# Yukarıdaki özeti SHA256SUMS ile karşılaştırdıktan sonra:
tar -xzf nect-aarch64-apple-darwin.tar.gz
mkdir -p ~/.local/bin
install -m 755 nect-aarch64-apple-darwin/nect ~/.local/bin/nect
```

Linux'ta özet için `sha256sum` kullanılabilir. Arşiv adları sürüm numarası
içermeyebilir; sürüm bilgisi arşivin `VERSION` dosyasındadır. PATH ayarı için
kabuğunuzun profil dosyasına şu satırı ekleyip yeni bir terminal açın:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## Kaynaktan kurulum

Güncel kararlı Rust ve platformun derleme/link araçları gerekir. `Cargo.toml`
minimum Rust sürümünü belirtir; bağımlılıklar daha yeni bir sürüm isteyebilir.
Windows'ta Rust MSVC toolchain ile Visual Studio Build Tools ve Windows SDK;
macOS'ta Xcode Command Line Tools; Linux'ta uygun C/link araçları gerekir.

```sh
git clone https://github.com/necatiprok1/nect.git
cd nect
cargo install --path .
```

Özellik seçmek veya kurmadan denemek için:

```sh
cargo install --path . --features full
cargo install --path . --features gui,db
cargo run -- run examples/hello.nct
```

## Lean, full ve isteğe bağlı özellikler

**Lean** dil çekirdeği, VM, JIT, C backend ve gömülü standart kütüphanedir.
**Full** buna yalnızca dil sunucusu, paket yöneticisi ve FFI ekler.

| Cargo özelliği | Sağladığı işlev |
| --- | --- |
| `net` | `http_get`, `http_post`, `http_request` |
| `server` | HTTP sunucusu ve routing |
| `db` | SQLite `db_*` fonksiyonları |
| `gui` | Yerel arayüz için `gui_*` fonksiyonları |
| `lsp` | `nect lsp` |
| `pkg` | `nect pkg` |
| `ffi` | `extern` bildirimleri |
| `full` | `lsp` + `pkg` + `ffi` |

`net`, `server`, `db` ve `gui`, `full` içinde **yoktur**. Bunlar için özel
kaynaktan derleme gerekir. Özellikler derleme zamanında seçilir; kurulu binary'ye
sonradan eklenmez. Eksik özellik çağrısı, gereken özelliği belirten bir hata verir.

## Kurulumu doğrulama

```sh
nect --version
nect doctor
printf 'print("Merhaba, Nect!")\n' > merhaba.nct
nect check merhaba.nct
nect run merhaba.nct
```

Son komut `Merhaba, Nect!` yazmalıdır. `nect doctor`, binary'nin özelliklerini
ve araçları raporlar. C derleyicisi eksik olsa da `nect run` çalışabilir.

## Sorun giderme

- **`nect: command not found`:** Yeni terminal açın; `~/.local/bin` dizininin
  PATH içinde olduğunu kontrol edin. Zsh için `source ~/.zshrc` kullanılabilir.
- **İndirme/ağ hatası:** Ağ ve proxy ayarlarını kontrol edin veya manuel kurun.
- **Checksum hatası:** Yeniden indirin. Hata sürerse bildirin; doğrulamayı
  atlayarak kurulum yapmayın.
- **Windows paketi bulunamadı:** Henüz hazır binary yayınlanmadı; kaynaktan derleyin.
- **`nect build` C derleyicisini bulamıyor:** Linux'ta `build-essential`, macOS'ta
  `xcode-select --install` veya Windows'ta uygun C derleme araçlarını kurun.
- **Program hatası:** Sözdizimi, runtime hata kataloğu ve engine farkları için
  [dil referansına](reference.md), kullanım örnekleri için [tutorial'a](tutorial.md)
  bakın.

## Kaldırma

Varsayılan macOS/Linux kurulumunda:

```sh
rm -f ~/.local/bin/nect
```

Kabuğun profil dosyasındaki `# added by the Nect installer` PATH bloğunu da
kaldırın. Özel bir dizine kurduysanız yalnızca oradaki Nect dosyasını kaldırın.
`cargo install` ile kurulduysa `cargo uninstall nect` kullanın.

## Yayın kontrolü: geliştiriciler

Yayın etiketi, kaynak commit'i, paket sürümü ve gerçek ekleri birlikte doğrulayın.
Her platform için lean/full arşivlerini açıp `nect --version` ve basit bir `.nct`
programını çalıştırın; `SHA256SUMS` değerlerini ve kurulum betiklerini kontrol edin.
Workflow'un hedeflediği bir platformun başarıyla yayınlandığını varsaymayın.
Windows desteğini ancak binary'ler gerçekten yayınlanıp temiz bir makinede
kurulum doğrulandıktan sonra kullanılabilir olarak belgeleyin.
