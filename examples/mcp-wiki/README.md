# MCP Server to read Wikipedia Article

### Test using MCP Inspector

1. Install venv and package dependencies:

```bash
uv sync
```

1. Activate your virtual environment:

```bash
source .venv/bin/activate
```

1. Run your server in development mode:

```bash
mcp dev src/mcp_wiki/server.py
```

1. Go to `http://localhost:5173` in your browser to open the MCP Inspector UI.

2. In the UI, you can click "Connect" to initialize your MCP server. Then click on "Tools" tab > "List Tools" and you should see the `read_wikipedia_article` tool.
   Then you can try to call the `read_wikipedia_article` tool with URL set to "<https://en.wikipedia.org/wiki/Bangladesh>" and click "Run Tool".

### Testing the CLI

1. Install your project locally:

```bash
uv pip install .
```

1. Check the executable in your virtual environment:

```bash
ls .venv/bin/  # Verify your CLI is available
```

1. Test the CLI:

```bash
mcp-wiki --help
```

You should see output similar to:

```plaintext
❯ mcp-wiki --help
usage: mcp-wiki [-h]

Gives you the ability to read Wikipedia articles and convert them to Markdown.

options:
 -h, --help  show this help message and exit
```
