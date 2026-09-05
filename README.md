<div align="center">

# Nexus Clipboard

**A fast, private, local-first clipboard manager for Windows that respects your RAM.**

[![Release](https://img.shields.io/github/v/release/nexouya/nexus-clipboard?color=6366f1&style=flat-square&label=latest%20release)](https://github.com/nexouya/nexus-clipboard/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D4?style=flat-square&logo=windows&logoColor=white)](https://github.com/nexouya/nexus-clipboard/releases/latest)
[![License](https://img.shields.io/badge/license-AGPL--3.0-purple?style=flat-square)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/engine-Rust%20%2B%20Tauri%202-dea584?style=flat-square&logo=rust)](https://v2.tauri.app/)
[![UI](https://img.shields.io/badge/ui-React%2019%20%2B%20Tailwind%20v4-61DAFB?style=flat-square&logo=react)](https://react.dev/)

<br />

<p align="center">
  <a href="#-quick-download"><b>Download (.exe)</b></a> •
  <a href="#-why-nexus"><b>Why Nexus?</b></a> •
  <a href="#-features"><b>Features</b></a> •
  <a href="#-shortcuts"><b>Shortcuts</b></a> •
  <a href="#-ai-providers"><b>AI Providers</b></a> •
  <a href="#fa-section"><b>🇮🇷 نسخه فارسی</b></a>
</p>

<br />

<img src="image/Nexus_en.jpg" alt="Nexus Clipboard Preview" width="100%" style="border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,0.25);" />

</div>

---

<details id="fa-section">
<summary><b>🇮🇷 برای باز کردن مستندات فارسی کلیک کنید (Click to expand Persian README)</b></summary>

<div dir="rtl" align="right">

<br/>

<div align="center">
  <img src="image/Nexous_fa.jpg" alt="پیش‌نمایش نکسوس کلیپ‌بورد" width="100%" style="border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,0.25);" />
</div>

<br/>

نکسوس (Nexus) یه کلیپ‌بورد منیجر سبک، فوق‌العاده سریع و کاملاً محلی (Local-First) برای ویندوزه. هدف ساخت نکسوس ساده بود: ابزار پیش‌فرض کلیپ‌بورد ویندوز تاریخچه محدودی داره و امکانات سرچش ضعیفه، برنامه‌های جانبی هم اکثراً با الکترون (Electron) ساخته شدن که ۵۰۰ تا ۸۰۰ مگابایت رم می‌بلعن و فن سیستم رو درگیر نگه می‌دارن، یا داده‌هات رو بدون اجازه به کلود می‌فرستن.

نکسوس با **Rust (Tauri v2)** در بک‌اند و **React 19** در فرانت‌اند ساخته شده؛ توی حالت بی‌کاری دقیقاً **۰٪ پردازنده** مصرف می‌کنه، مصرف رمش زیر **۳۵ مگابایته**، و هیچ دیتایی بدون درخواست صریح خودت سیستم رو ترک نمی‌کنه.

### ⚡ مقایسه نکسوس با رقبا

| ویژگی | کلیپ‌بورد ویندوز (Win+V) | برنامه‌های الکترونی (Ditto/CopyQ و...) | نکسوس (Nexus) |
|---|:---:|:---:|:---:|
| **مصرف حافظه رم** | حدود ۱۵-۳۰ مگابایت | ۴۰۰ الی ۸۰۰ مگابایت | **حدود ۳۵ مگابایت** |
| **مصرف پردازنده در حالت Idle** | کم | ۱٪ تا ۳٪ مداوم (پایش ثانیه‌ای) | **۰.۰٪ (رویدادمحور با Win32 API)** |
| **محدودیت ذخیره تاریخچه** | حداکثر ۲۵ آیتم | نامحدود (معمولاً با افت سرعت) | **بیش از ۵۰۰,۰۰۰ آیتم بدون افت سرعت** |
| **سرعت جستجو در حجم بالا** | ساده / کند | کند با افزایش دیتابیس | **زیر ۵ میلی‌ثانیه (SQLite FTS5 + BM25)** |
| **امنیت پسوردها و توکن‌ها** | متن خام در دیتابیس | اکثراً بدون رمزنگاری | **رمزنگاری XChaCha20-Poly1305 + ماسک خودکار** |
| **هوش مصنوعی اختیاری** | ندارد | وابسته به افزونه | **پشتیبانی مستقیم از Claude, OpenAI, Ollama, DeepSeek** |

### 🎯 مهم‌ترین امکانات
- **لانچر سریع و پیست مستقیم**: با زدن کلید `Ctrl+Shift+V` لانچر شناور باز میشه و متن انتخاب‌شده مستقیماً توی برنامه‌ای که بودی پیست میشه.
- **پیست آیتم قبلی بدون باز کردن پنجره**: با زدن `Ctrl+Shift+B` آیتم قبلی بلافاصله پیست میشه (عالی برای کپی/پیست متناوب دو مقدار).
- **تشخیص خودکار و هوشمند محتوا**: تشخیص کدهای برنامه‌نویسی (۱۵ زبان)، لینک‌ها، رنگ‌ها، JSON، ایمیل و شماره تماس.
- **محافظت از رمزها (Quarantine)**: توکن‌های گیت‌هاب، سکرت‌های AWS، کلیدهای خصوصی و شماره کارت‌ها شناسایی میشن، پیش‌نمایششون ستاره‌دار میشه و متنشون به صورت رمزنگاری‌شده ذخیره میشه تا توی سرچ لو نرن.
- **۲۱ ابزار تغییر و تبدیل متن کاملاً آفلاین**: تبدیل فرمت‌های `camelCase`، `snake_case`، انکود و دیکود Base64 و URL، مرتب‌سازی خطوط، و فرمت JSON.
- **هوش مصنوعی بدون قفل روی یک سرویس**: می‌تونی مدل‌های محلی مثل Ollama یا LM Studio رو با Base URL دلخواه وصل کنی یا از کلیدهای OpenAI و Claude استفاده کنی.

### 📥 دانلود و نصب
- دانلود مستقیم نسخه نصبی: [**Nexus.Clipboard_1.0.0_x64-setup.exe**](https://github.com/nexouya/nexus-clipboard/releases/latest/download/Nexus.Clipboard_1.0.0_x64-setup.exe)
- پکیج سازمانی MSI: [**Nexus.Clipboard_1.0.0_x64_en-US.msi**](https://github.com/nexouya/nexus-clipboard/releases/latest/download/Nexus.Clipboard_1.0.0_x64_en-US.msi)

---

</div>
</details>

---

## 📥 Quick Download

Pre-built releases for Windows 10/11 (64-bit). No installer hoops, signed with checksums:

| Package | Type | Size | Direct Download |
|---|---|---|:---:|
| **Standard Installer** | `.exe` (NSIS) | ~4.1 MB | [**Download Setup.exe**](https://github.com/nexouya/nexus-clipboard/releases/latest/download/Nexus.Clipboard_1.0.0_x64-setup.exe) |
| **Enterprise / Admin** | `.msi` (Windows Installer) | ~5.4 MB | [**Download .msi**](https://github.com/nexouya/nexus-clipboard/releases/latest/download/Nexus.Clipboard_1.0.0_x64_en-US.msi) |
| **Release Notes & Hashes** | Release Page | - | [**View Release v1.0.0**](https://github.com/nexouya/nexus-clipboard/releases/latest) |

---

## 💡 Why Nexus?

Most Windows users put up with one of two extremes:
1. The native Windows clipboard (`Win+V`), which caps out at 25 items, loses history unexpectedly, and lacks full-text search.
2. Third-party clipboard managers built on Electron that stay resident in memory taking **400MB to 800MB of RAM** and poll the clipboard every few hundred milliseconds.

Nexus was written from scratch in **Rust** to feel like an OS primitive:

```
                  Memory Usage (Cold / Idle)
Nexus Clipboard   ██ 34 MB
Windows Win+V     █ 22 MB
Typical Electron  ████████████████████████████████ 550 MB+
```

- **Zero-poll architecture**: Listens directly to Windows `WM_CLIPBOARDUPDATE` messages via `AddClipboardFormatListener`. If you aren't copying anything, Nexus uses **0.0% CPU**.
- **Keyset-paginated SQLite FTS5**: Search queries never use SQL `OFFSET`. Searching across 200,000 entries feels as instant as searching across 10.
- **BLAKE3 Content Addressing**: Copied the same 15MB screenshot 20 times? It's stored once. Deduplication happens before disk writes.
- **Zero Cloud Leakage**: No telemetry, no usage metrics, no auto-sync servers. Everything lives in `%APPDATA%\dev.nexus.clipboard`.

---

## ⚡ Features at a Glance

<details open>
<summary><b>1. Fast Capture & Smart Classification</b></summary>
<br/>

- **Auto-type detection**: Automatically detects and styles code snippets (with syntax highlight across 15+ languages), URLs, colors (HEX/RGB), JSON payloads, emails, files, and rich text.
- **Automatic credential protection**: AWS keys, GitHub tokens (`ghp_`), private keys, JWTs, and card numbers are masked immediately, encrypted at rest with **XChaCha20-Poly1305**, and kept out of the search index.
- **Built-in ignore list**: Prevents capturing from password managers like 1Password, Bitwarden, KeePassXC, Dashlane, and Proton Pass out of the box.
</details>

<details open>
<summary><b>2. Keyboard-Driven Workflow</b></summary>
<br/>

- **Quick Picker (`Ctrl+Shift+V`)**: A floating spotlight window positioned at your cursor or centered. Tap Enter to paste directly into your active app.
- **Ghost Paste (`Ctrl+Shift+B`)**: Pastes the *previous* clipboard item without opening any UI. Essential for switching back and forth between two fields.
- **Command Palette (`Ctrl+Shift+P` / `Ctrl+K`)**: Instant access to every setting, tag, filter, and action.
</details>

<details>
<summary><b>3. 21 Built-in Offline Text Tools</b></summary>
<br/>

Transform any copied text without opening a browser tab:
- **Case formatting**: `camelCase`, `snake_case`, `kebab-case`, `Title Case`, `UPPERCASE`, `lowercase`, `slugify`.
- **Developer tools**: Format JSON, Minify JSON, Base64 encode/decode, URL encode/decode.
- **Line operations**: Sort lines, reverse lines, deduplicate unique lines, strip common indentation, count words & characters.
</details>

---

## 🤖 AI Providers

Nexus includes optional AI transformations (*Summarize*, *Explain Code*, *Translate*). **It is completely disabled by default.**

When turned on, only the single entry you run a transform on is sent — never your clipboard history. We support:

- **Claude (Anthropic)**: Native Anthropic API integration.
- **OpenAI**: Native OpenAI API integration.
- **Custom / Local Endpoints**: Any OpenAI-compatible server or proxy:
  - **Ollama**: Local models via `http://localhost:11434/v1` (no API key needed)
  - **DeepSeek**: Cloud API or self-hosted instances
  - **Any custom gateway**: OpenRouter, LM Studio, LocalAI, vLLM, or self-hosted endpoints.

Configure it anytime in `Settings (Ctrl+,) → Intelligence`.

---

## ⌨️ Shortcuts

### Global (System-wide)
| Shortcut | What it does |
|---|---|
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>V</kbd> | Open Quick Picker Launcher |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>B</kbd> | Paste previous item in background |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>P</kbd> | Open Command Palette |

### Inside Nexus
| Shortcut | Action |
|---|---|
| <kbd>/</kbd> or <kbd>Ctrl</kbd> + <kbd>F</kbd> | Jump to search box |
| <kbd>↑</kbd> <kbd>↓</kbd> or <kbd>j</kbd> <kbd>k</kbd> | Navigate list |
| <kbd>Enter</kbd> | Copy & paste active item |
| <kbd>P</kbd> | Toggle Pin |
| <kbd>Del</kbd> | Move to Trash |
| <kbd>Ctrl</kbd> + <kbd>,</kbd> | Open Settings |
| <kbd>Esc</kbd> | Close window / Clear search |

---

## 🛠️ Building from Source

### Prerequisites
- **Windows 10 / 11** (64-bit)
- **Rust 1.80+**: Install via [rustup.rs](https://rustup.rs)
- **Node.js 20+** & **pnpm**: `npm install -g pnpm`
- **Visual Studio Build Tools** (C++ workload)

### Build Steps

```bash
# 1. Clone repo
git clone https://github.com/nexouya/nexus-clipboard.git
cd nexus-clipboard

# 2. Install dependencies
pnpm install

# 3. Run in dev mode with hot-reload
pnpm app:dev

# 4. Build standalone production release (.exe + .msi)
pnpm app:build
```

---

## 🔒 Security Architecture

- **Vault Key Derivation**: Uses **Argon2id** with memory-hard parameters to derive keys from machine state or user passphrase.
- **Data Encryption**: **XChaCha20-Poly1305** authenticated encryption with 192-bit random nonces per item.
- **Search Sanitization**: Credential text is never indexed by SQLite FTS5; only masked placeholder tokens (`ghp_••••••••`) enter search tables.
- **Webview Hardening**: Right-click context menus, F5 refreshes, and dev shortcuts are disabled in production builds.

---

## 📄 License

Nexus Clipboard is open-source software licensed under the **[GNU Affero General Public License v3.0 (AGPL-3.0)](LICENSE)**.
