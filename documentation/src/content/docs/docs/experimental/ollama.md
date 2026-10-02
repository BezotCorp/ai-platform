---
title: Ollama Tool Shim

sidebar:
  label: Ollama Tool Shim
  order: 2
---

:::warning Experimental Feature
Ollama tool shim is an experimental feature. Behavior and configuration may change in future releases.
:::

The Ollama tool shim enables tool calling for models that don't natively support it. For full setup instructions, configuration options, and troubleshooting, see the **[Tool Shim guide](/guides/tool-shim)**.

#### Quick start

1. Install and start [Ollama](https://ollama.com/download)
2. Pull the default interpreter model:
   ```bash
   ollama pull mistral-nemo
   ```
3. Start goose with the shim enabled:
   ```bash
   GOOSE_TOOLSHIM=true goose session
   ```
