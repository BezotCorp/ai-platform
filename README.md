<div align="center">

# BCAIP

_your native open source AI agent platform — desktop app, CLI, API, and SDKs_

<p align="center">
  <a href="https://opensource.org/licenses/Apache-2.0"
    ><img src="https://img.shields.io/badge/License-Apache_2.0-blue.svg"></a>
  <a href="https://github.com/BezotCorp/ai-platform/actions/workflows/ci.yml"
    ><img src="https://img.shields.io/github/actions/workflow/status/BezotCorp/ai-platform/ci.yml?branch=main" alt="CI"></a>
</p>

</div>

BCAIP is a general-purpose AI agent platform that runs on your machine. Use it for code, research, writing, automation, data analysis, and agent-driven workflows.

It includes a native desktop application, a CLI, APIs, SDKs, and support for agent integrations. The core platform is built in Rust for performance and portability.

BCAIP supports multiple model providers and local inference, including providers such as Anthropic, OpenAI, Google, Ollama, OpenRouter, Azure, and Bedrock. It also integrates with the Model Context Protocol (MCP) and Agent Client Protocol (ACP).

The project is maintained by BezotCorp.

# Get started

Install the CLI from the latest stable release:

```bash
curl -fsSL https://github.com/BezotCorp/ai-platform/releases/download/stable/download_cli.sh | bash
```

# Repository

- [Source code](https://github.com/BezotCorp/ai-platform)
- [Issues](https://github.com/BezotCorp/ai-platform/issues)
- [Releases](https://github.com/BezotCorp/ai-platform/releases)
- [Governance](https://github.com/BezotCorp/ai-platform/blob/main/GOVERNANCE.md)
- [Custom Distributions](https://github.com/BezotCorp/ai-platform/blob/main/CUSTOM_DISTROS.md)

# Components

BCAIP contains:

- the core Rust agent platform;
- the BCAIP CLI;
- the desktop application;
- ACP support;
- MCP integrations;
- SDKs and developer tooling;
- optional experimental capabilities such as roaming agents.

# License

BCAIP is distributed under the Apache License 2.0.
