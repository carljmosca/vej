# `vej-playground` ⚡

> **Interactive System 1 Decision Playground for `vej` and TypeSafe AI `Jev`**  
> Test typed decisions, calibrate softmax probabilities, and run side-by-side performance benchmarks.

---

## 🚀 Features

- **Multi-Target Switcher:** Run decisions against:
  - ⚡ **Local `vej` Daemon** (`http://127.0.0.1:8080`)
  - ☁️ **TypeSafe AI Hosted Jev** (`https://api.typesafe.ai`)
  - ⚖️ **Dual Benchmark Mode** (executes both in parallel and compares latency, confidence, and output tokens side-by-side!)
- **Dynamic Visual Form Builder:**
  - Support for primitives: **`Choice`** (multi-class) and **`Noul`** (boolean yes/no)
  - Interactive criteria editor (add/remove options, descriptions, instructions)
- **Bidirectional JSON Synchronization:**
  - Switch seamlessly between the visual form builder and the raw JSON payload editor without losing state.
- **Preset Library:**
  - One-click templates: Customer Support Ticket Triage, Security Perimeter Firewall, Autonomous Agent Error Recovery, Noul PII Compliance, Urgency & SLA.
- **Metrics & Probability Visualization:**
  - Real-time confidence scores and ranked probability distribution bars
  - Latency breakdown (inference vs. network round-trip)
  - Zero-decode token tracking
- **Instant Code Export:**
  - Export copy-ready snippets in **cURL**, **Python** (`requests`), **TypeScript** (`fetch`), and **Rust** (`reqwest`).

---

## 💻 How to Run

### Method 1: Served Directly by `vej serve` (Zero Setup)
`vej serve` serves this playground directly out of the box with CORS enabled!

1. Start the daemon from the repository root:
   ```bash
   vej serve -m llama-3.2-1b
   ```
2. Open your browser:
   ```text
   http://127.0.0.1:8080/playground
   ```

### Method 2: Standalone Vite Dev Server (Hot Module Reload)
Run the playground as a standalone modern web application with Vite:

```bash
cd playground

# Install dependencies (only required for Vite dev server)
npm install

# Start Vite dev server with hot module reload
npm run dev
```
The playground will open at `http://localhost:3000`.

### Method 3: Direct Browser File / Static Server
Because the application uses native ES modules and zero build artifacts, you can also launch it with any static server or open it directly:

```bash
# Python
python3 -m http.server 3000 --directory playground

# Or open in default browser on macOS
open playground/index.html
```
