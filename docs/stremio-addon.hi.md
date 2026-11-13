# Stremio में MovieBox (`moviebox-stremio`)

[English](stremio-addon.md) • **हिन्दी**

`moviebox-stremio` इस repo का एक अलग प्रोग्राम है। यह MovieBox को एक
**Stremio addon** की तरह आपके अपने Android फ़ोन या टैबलेट पर चलाता है।
इससे आप MovieBox की फ़िल्में और सीरीज़ Stremio में देख सकते हैं: फ़ोन,
Android TV, PC या ब्राउज़र (web.stremio.com) में। किसी paid server की ज़रूरत
नहीं है।

Stremio में आपको क्या मिलेगा:

| Stremio में | कहाँ से आता है |
| --- | --- |
| Board / Discover पर **MovieBox Trending** (फ़िल्में + सीरीज़) | MovieBox होमपेज |
| **Search** करने पर "MovieBox" की लाइन | MovieBox search |
| MovieBox टाइटल की details, seasons और episodes | MovieBox details |
| **Stremio की आम फ़िल्मों पर streams** (Cinemeta / IMDb `tt…` पेज) | नाम + साल से MovieBox में मिलान |
| हर डब अलग stream के रूप में (Hindi, Tamil, …) | MovieBox dubs |
| हर stream के साथ subtitles | MovieBox captions |

## यह काम कैसे करता है

```
 Stremio (TV / फ़ोन / ब्राउज़र)
        │  https://xxxx.trycloudflare.com/manifest.json
        ▼
 Cloudflare quick tunnel (मुफ़्त, addon को https:// लिंक देता है)
        │
        ▼
 घर पर रखा Android फ़ोन/टैबलेट, Termux में
   cloudflared  ──►  moviebox-stremio (http://127.0.0.1:7000)  ──►  MovieBox
   (दोनों को `mbx` script चलाए रखती है, Termux:Boot इसे फ़ोन चालू होते ही शुरू करता है)
```

- दूसरे डिवाइस से Stremio सिर्फ़ **https** वाला addon लिंक मानता है। सादा
  `http://192.168.x.x:7000` लिंक डालने पर *"Failed to fetch"* आता है।
  Cloudflare tunnel यही https लिंक देता है।
- वीडियो उसी फ़ोन/टैबलेट से होकर आता है, क्योंकि addon वहीं MovieBox के
  ज़रूरी headers जोड़ता है। इसलिए देखते समय वह डिवाइस चालू और इंटरनेट से
  जुड़ा रहना चाहिए।

---

## Setup: स्टेप-बाय-स्टेप

स्टेप 1–2 हर GitHub अकाउंट के लिए एक बार, और स्टेप 3–9 हर फ़ोन/टैबलेट के
लिए एक बार करने हैं।

### स्टेप 1: Repo को Fork करें

GitHub पर यह repository खोलें और **Fork** दबाएँ। आपका fork होगा
`https://github.com/YOURNAME/MovieBox-Tui`। नीचे हर कमांड में `YOURNAME` की
जगह अपना GitHub username लिखें।

### स्टेप 2: GitHub पर addon build करें

1. अपने fork में **Actions** टैब खोलें। अगर GitHub पूछे, तो
   **I understand my workflows, go ahead and enable them** दबाएँ।
2. बाईं तरफ़ **Stremio addon** चुनें → **Run workflow** → **Run workflow**।
3. हरा ✓ आने तक रुकें (लगभग 5–10 मिनट)।

इससे `stremio-addon` नाम की release बनती है, जिसमें तैयार files होती हैं:
ज़्यादातर फ़ोन के लिए `android-arm64`, पुराने 32-bit डिवाइस के लिए
`android-armv7`, और PC के लिए `linux-x86_64`। इसके बाद `main` पर code बदलने
वाला हर push इसे अपने-आप दोबारा build करता है।

### स्टेप 3: Android ऐप्स इंस्टॉल करें

जिस फ़ोन/टैबलेट पर addon चलाना है, उस पर [F-Droid](https://f-droid.org) से
(या हर ऐप की GitHub release से) इंस्टॉल करें:

- **Termux**: ज़रूरी है। Google Play वाला Termux न लें, वह पुराना है।
- **Termux:Boot**: वैकल्पिक है, फ़ोन restart होने पर addon अपने-आप चालू
  करता है। इंस्टॉल करने के बाद इसे **एक बार खोलें**, वरना Android इसे कभी
  नहीं चलाता।

### स्टेप 4: Termux को files सेव करने की permission दें

Termux खोलें और चलाएँ:

```bash
termux-setup-storage
```

**Allow** दबाएँ। इससे addon अपना मौजूदा लिंक आपके **Download** फ़ोल्डर में
लिख पाता है, ताकि आप बिना कोई कमांड चलाए लिंक देख सकें।

### स्टेप 5: Addon इंस्टॉल करें

Termux में ही:

```bash
pkg update -y && pkg install -y curl
curl -fsSL https://raw.githubusercontent.com/YOURNAME/MovieBox-Tui/main/install-stremio-addon.sh \
  | MBX_REPO=YOURNAME/MovieBox-Tui bash
```

Installer ये सब इंस्टॉल करता है:

| क्या | कहाँ | किसलिए |
| --- | --- | --- |
| `moviebox-stremio` | `$PREFIX/bin/` | addon server |
| `cloudflared` | Termux package | मुफ़्त https tunnel |
| `mbx` | `$PREFIX/bin/` | दोनों को चलाता है, बंद होने पर फिर से चालू करता है, लिंक सेव करता है |
| boot script | `~/.termux/boot/start` | डिवाइस चालू होते ही `mbx start` चलाता है |

अगर पहले से कोई boot script थी, तो वह `~/.termux/boot/start.bak` नाम से
सेव हो जाती है।

### स्टेप 6: Addon चालू करें और लिंक लें

```bash
mbx start
```

लगभग 20 सेकंड रुकें, फिर:

```bash
mbx link
```

ऐसा लिंक दिखेगा:

```
https://word-word-word-word.trycloudflare.com/manifest.json
```

यही लिंक **Download/moviebox-stremio-link.txt** फ़ाइल में भी सेव होता है।

### स्टेप 7: Stremio में addon इंस्टॉल करें

स्टेप 6 वाला https लिंक इस्तेमाल करें। कोई एक तरीका चुनें:

- **Stremio ऐप (फ़ोन या TV):** **Addons** → ऊपर search box में लिंक paste
  करें → **Install**।
- **ब्राउज़र:** <https://web.stremio.com> खोलें, **sign in** करें,
  **Addons** → **Add addon** → लिंक paste करें → **Install**।

Addons आपके Stremio अकाउंट से sync होते हैं। इसलिए sign in करके एक बार
इंस्टॉल करने पर addon उस अकाउंट के हर डिवाइस पर आ जाता है। TV पर लंबा लिंक
टाइप करना मुश्किल है, इसलिए ब्राउज़र वाला तरीका सबसे आसान है।

### स्टेप 8: कुछ देखें

Stremio में कोई भी फ़िल्म या सीरीज़ खोलें। Streams में **MovieBox 1080p**,
**MovieBox 480p** जैसे विकल्प दिखेंगे, साथ में भाषा (Original Audio, Hindi
dub, …), साइज़ और codec भी। कोई एक चुनें, subtitles के साथ चल जाएगी। Board
पर **MovieBox Trending** और search में **MovieBox** वाले नतीजे भी काम करते
हैं।

**ब्राउज़र** में **h264** वाली stream चुनें, क्योंकि कई ब्राउज़र **hevc**
stream नहीं चला पाते। Stremio ऐप में दोनों चलती हैं।

### स्टेप 9: Background में चलता रहे

Android बैटरी बचाने के लिए background ऐप्स बंद कर देता है। इसे रोकने के लिए:

- **Settings → Apps → Termux → Battery → Unrestricted** (हर फ़ोन में नाम थोड़ा
  अलग हो सकता है; Xiaomi में *Battery saver → No restrictions*, और Termux व
  Termux:Boot के लिए **Autostart** भी चालू करें)।
- अगर यही आपका हमेशा चालू रहने वाला डिवाइस है, तो उसे चार्जिंग पर और Wi-Fi
  से जुड़ा रखें।

`mbx start` पहले से Termux का wake lock ले लेता है (Termux notification में
"wake lock held" दिखता है)।

---

## Restart के बाद: लिंक बदल जाता है

Quick tunnel हर बार चालू होने पर **नया** `trycloudflare.com` पता लेता है:
reboot के बाद, `mbx restart` के बाद, या जब `mbx` बंद पड़े tunnel को बदलता है।
जब Stremio में MovieBox की streams आनी बंद हो जाएँ:

1. **Download/moviebox-stremio-link.txt** खोलें (या `mbx link` चलाएँ)।
2. वह लिंक Stremio में दोबारा इंस्टॉल करें, स्टेप 7 की तरह।

नया इंस्टॉल पुराने को **अपने-आप बदल देता है** (addon ID वही है), इसलिए कुछ
uninstall करने की ज़रूरत नहीं है।

कभी न बदलने वाला लिंक चाहिए तो मुफ़्त Cloudflare अकाउंट पर एक domain और
*named* tunnel चाहिए (`cloudflared tunnel login`, फिर
`cloudflared tunnel create` …), या addon किसी server पर host करें
(`render.yaml` देखें)। दोनों वैकल्पिक हैं।

---

## रोज़ के कमांड

| कमांड | क्या करता है |
| --- | --- |
| `mbx status` | addon चल रहा है? tunnel चल रहा है? मौजूदा लिंक |
| `mbx link` | मौजूदा Stremio लिंक दिखाता है |
| `mbx start` | addon + tunnel को background में चालू (या फिर से चालू) करता है |
| `mbx stop` | दोनों को बंद करता है |
| `mbx restart` | बंद करके फिर चालू करता है (**लिंक बदल जाता है**) |
| `mbx update` | आपके fork से नया build इंस्टॉल करता है; **लिंक वही रहता है** |

Logs `~/.mbx/` में हैं: `addon.log` (addon), `tunnel.log` (cloudflared) और
`mbx.log` (चालू होना, restart और health check)।

---

## समस्या और हल

| समस्या | हल |
| --- | --- |
| Addon इंस्टॉल करते समय *"Failed to fetch"* | आपने `http://` लिंक डाला है। `mbx link` वाला `https://….trycloudflare.com/manifest.json` लिंक डालें। |
| हर टाइटल पर *"No streams were found"* | 1. `mbx status`: दोनों *running* होने चाहिए। 2. Stremio में डाला लिंक `mbx link` वाले लिंक से मिलना चाहिए; न मिले तो नया लिंक इंस्टॉल करें (स्टेप 7)। 3. Commit `de34c3d` से पुराने builds stream में एक field (`title`) भेजते थे, जिसकी वजह से Stremio सारी MovieBox streams छोड़ देता था। `mbx update` चलाएँ। |
| किसी एक टाइटल पर MovieBox stream नहीं | MovieBox में उसका नाम अलग हो सकता है। Stremio में search करें और **MovieBox** वाली लाइन से खोलें। कोई टाइटल पहली बार खोलने पर कुछ सेकंड लग सकते हैं; दोबारा खोलें। |
| ब्राउज़र में stream नहीं चलती | शायद वह **hevc** है। **h264** वाली stream चुनें, या Stremio ऐप इस्तेमाल करें। |
| Stream चलकर रुक जाती है | CDN लिंक expire हो गया। Player बंद करके stream फिर से चुनें। |
| Screen बंद होने पर सब रुक जाता है | स्टेप 9: Termux के लिए बैटरी unrestricted करें, और Xiaomi फ़ोन पर Autostart चालू करें। |
| Restart के बाद कुछ चालू नहीं होता | Termux:Boot ऐप एक बार खोलें, फिर restart करें। या हाथ से चालू करें: `mbx start`। |
| Download में लिंक वाली फ़ाइल नहीं | `termux-setup-storage` चलाएँ (स्टेप 4), फिर `mbx restart`। |
| ज़्यादा जानकारी चाहिए | `tail -n 50 ~/.mbx/addon.log`। हर request का log देखने के लिए: `mbx stop`, फिर `MBX_DEBUG=1 moviebox-stremio`। |

---

## Reference

### Addon को हाथ से चलाना

`mbx` सबसे आसान तरीका है। Server को सीधे चलाने के लिए (जैसे debug करते समय):

```bash
termux-wake-lock
moviebox-stremio
```

यह दिखाता है:

```
  Same device:   http://127.0.0.1:7000/manifest.json
  Other devices: http://192.168.1.23:7000/manifest.json
```

`http://127.0.0.1:7000/manifest.json` लिंक **उसी डिवाइस** के Stremio में
बिना tunnel के काम करता है।

Options (flags या environment variables):

| Flag | Env | Default | मतलब |
| --- | --- | --- | --- |
| `--port` | `MBX_PORT` | `7000` | Port |
| `--host` | `MBX_HOST` | `0.0.0.0` | Bind address (`127.0.0.1` = सिर्फ़ यही डिवाइस) |
| `--public-url` | `MBX_PUBLIC_URL` | – | लिंक में डालने वाला base URL (tunnel/reverse proxy के लिए) |
| `--no-proxy` | `MBX_PROXY=0` | proxy चालू | Stremio को सीधे CDN लिंक + `proxyHeaders` देता है |
| – | `MBX_DEBUG=1` | बंद | हर request का log |

`mbx` default port 7000 मानकर चलता है।

### GitHub की जगह फ़ोन पर build करना

धीमा है (पहली बार 15–40 मिनट), लेकिन fork की ज़रूरत नहीं:

```bash
pkg update && pkg install -y rust git clang make binutils
git clone https://github.com/YOURNAME/MovieBox-Tui.git
cd MovieBox-Tui
cargo build --release --bin moviebox-stremio
cp target/release/moviebox-stremio $PREFIX/bin/
```

फिर `cloudflared` और `mbx` खुद इंस्टॉल करें:

```bash
pkg install -y cloudflared
cp scripts/termux/mbx $PREFIX/bin/mbx && chmod 755 $PREFIX/bin/mbx
```

### Android TV पर ही चलाना

TV पर Termux इंस्टॉल करें (F-Droid से sideload) और वहीं स्टेप 3–9 करें। उसी
डिवाइस पर Stremio `http://127.0.0.1:7000/manifest.json` भी इस्तेमाल कर सकता
है, tunnel की ज़रूरत नहीं।
