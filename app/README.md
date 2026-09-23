# Hesap Makinesi

macOS tarzı hesap makinesi uygulaması. Nect dili ve `std/ui.nct` ile üretilmiş tarayıcı tabanlı arayüze sahiptir.

## Çalıştırma

```bash
./target/release/nect run app/calculator.nct
```

Uygulama tarayıcıda açılır. Tuşlayıcıya veya klavye ile (rakamlar, operatörler, Enter, Backspace, Escape) kontrol edebilirsiniz.

## Özellikler

- Temel aritmetik: +, -, ×, ÷
- Onluk ve virgül girişi
- İşaret değiştirme (±)
- Geri alma (⌫) ve temizleme (C)
- Klavye desteği
- Hata yönetimi (sıfıra bölünme, geçersiz ifade)
