# Nect

Nect, sade sözdizimine sahip dinamik bir betik dilidir. Programlar varsayılan
olarak bytecode VM üzerinde çalışır; kanıtlanabilir biçimde sayısal ve tekrarlanan
kod Cranelift JIT ile yerel koda derlenir. `nect build`, desteklenen sayısal alt
küme için C üzerinden bağımsız çalıştırılabilir dosya üretir.

```nct
let name = "Nect"
let values = [3, 1, 2]

fn mean(items) {
    return sum(items) / len(items)
}

print("Merhaba, ${name}!")
for value in sort(values) {
    print(value)
}
print("Ortalama: ${mean(values)}")
```

## Kurulum

**macOS ve Linux** — hazır binary; Rust veya C derleyicisi gerekmez:

```sh
curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh
```

Betik indirmeyi `SHA256SUMS` ile doğrular ve kullanıcı `PATH`'ini ayarlar.
İsterseniz çalıştırmadan önce betiği indirip inceleyin. Kurulumdan sonra yeni
bir terminal açın.

[v1.0.0 yayını](https://github.com/necatiprok1/nect/releases/tag/v1.0.0), macOS
ARM64/Intel ve Linux ARM64/x64 için lean/full arşivlerini içerir.
**Windows binary'leri henüz yayınlanmadı**; `install.ps1` dosyasının bulunması
Windows kurulumunun kullanılabilir olduğu anlamına gelmez. Windows'ta şimdilik
kaynaktan derleme gerekir. Yayın etiketi `v1.0.0` olsa da bu kaynağın paket
sürümü `0.1.0` olduğundan `nect --version` bu değeri gösterebilir.

Ayrıntılar: [kurulum, doğrulama ve sorun giderme](docs/KURULUM.md).

## İlk program

`hello.nct` dosyasına `print("Merhaba, Nect!")` yazın:

```sh
nect --version
nect doctor
nect run hello.nct
nect check hello.nct          # Çalıştırmadan sözdizimini denetle
nect disasm hello.nct         # Bytecode ve JIT kararları
nect run --interp hello.nct   # Referans yorumlayıcı
```

Sayılar, UTF-8 metinler, boolean, `null`, diziler ve sıralı map'ler;
fonksiyonlar, döngüler, string interpolation ve `x.f(y)` metot şekeri desteklenir.
Modüller `import "helpers.nct"` ile alınır. `std/` kütüphanesi binary içine
gömülüdür; çalıştırmak için depo veya ayrı bir runtime gerekmez.

## Derleme seçenekleri

Varsayılan **lean** derleme dil çekirdeği, VM, JIT, C backend ve gömülü standart
kütüphaneyi içerir. **full**, yalnızca `lsp` + `pkg` + `ffi` özelliklerini ekler;
HTTP, veritabanı ve yerel GUI ayrıca seçilir.

| Cargo özelliği | Sağladığı işlev |
| --- | --- |
| `lsp` | `nect lsp` dil sunucusu |
| `pkg` | `nect pkg` paket yöneticisi |
| `ffi` | `extern` ile native kütüphane çağrıları |
| `net` | HTTP istemcisi |
| `server` | HTTP sunucusu |
| `db` | SQLite |
| `gui` | Yerel GUI |
| `full` | `lsp`, `pkg`, `ffi` |

Kaynaktan kurulum için güncel kararlı Rust ve platformun derleme araçları gerekir:

```sh
git clone https://github.com/necatiprok1/nect.git
cd nect
cargo install --path .
# Alternatif özellik kümeleri:
cargo install --path . --features full
cargo install --path . --features gui,db
```

## Bağımsız executable

```sh
nect build hello.nct -o hello
nect build hello.nct --emit-c
```

`nect build` sistem C derleyicisini kullanır. Her Nect programı bu backend'e
uygun değildir: desteklenmeyen bir özellik kullanılıyorsa nedenini bildirir ve
build başarısız olur; programı `nect run` ile çalıştırabilirsiniz. JIT ve C
backend kapsamı için [dil referansına](docs/reference.md) bakın.

## Belgeler ve örnekler

- [Kurulum](docs/KURULUM.md) — platformlar, özellikler, PATH ve sorun giderme.
- [Tutorial](docs/tutorial.md) — dili adım adım öğrenme ve örnek çıktılar.
- [Dil referansı](docs/reference.md) — gramer, built-in'ler, CLI ve engine farkları.
- [API](docs/API.md) — kaynaklardan üretilen API belgesi.
- [Bellek modeli](docs/memory.md) ve [güvenlik sınırları](docs/security.md).
- [examples/](examples/) — küçük programlar, terminal hesap makinesi ve browser UI.

`std/ui.nct` browser arayüzü için korunur. `examples/webapp.nct` browser açar;
`calculator.nct` etkileşimlidir. Bunları gözetimsiz smoke test'e dahil etmeyin.
Nect programları sandbox içinde çalışmaz; yalnızca güvendiğiniz kodu çalıştırın.

## Geliştirme

```sh
cargo build
cargo build --features full
cargo build --all-features
cargo test --lib
cargo test --lib --all-features
cargo clippy --all-targets
cargo run -- run examples/hello.nct
```

Unit test'ler Rust modüllerindeki `#[cfg(test)]` bölümlerinde tutulur.
Runtime değişikliklerinde interpreter, JIT kapalı VM ve varsayılan VM çıktılarını
karşılaştırın. Mimari kurallar ve doğrulama akışı: [AGENTS.md](AGENTS.md).
