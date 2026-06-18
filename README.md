# Pipetable

Query local CSV, Parquet, and JSON files with natural language or SQL. Works as an MCP server (RooCode, Cursor, Claude Code, Copilot) and as a standalone CLI. Powered by DuckDB. Files never leave your machine.

## Install

**macOS / Linux**
```sh
curl -fsSL https://pipetable.com/install | sh
```

**Windows (PowerShell)**
```powershell
irm https://pipetable.com/install.ps1 | iex
```

**cargo install**
```sh
cargo install pipetable
```

## Usage

### Interactive REPL

```sh
pipetable ~/data/
```

Type natural language or SQL at the `>` prompt. Requires [Ollama](https://ollama.com) for NL queries — SQL always works without it.

```
> show me total revenue by region
Thinking.....
SELECT region, SUM(revenue) AS total FROM sales GROUP BY region

region  total
--------------
EU      141000
US       32000
APAC     17000

> SELECT name, MAX(revenue) FROM sales GROUP BY name ORDER BY 2 DESC LIMIT 1
1 row(s)
name   MAX(revenue)
--------------------
Carol  91000
```

Dot commands:

| Command | Description |
|---|---|
| `.scan <path>` | Load a folder or file |
| `.datasets` | List loaded datasets |
| `.schema <name>` | Show schema for a dataset |
| `.models` | List available Ollama models |
| `.model <name>` | Switch model |
| `.help` | Show help |
| `.quit` | Exit |

### One-shot query

```sh
pipetable ask "who has the highest revenue?" ~/data/
pipetable ask "SELECT * FROM sales LIMIT 5" ~/data/
```

### MCP server

```sh
pipetable mcp
```

Or with a default folder pre-loaded:

```sh
pipetable ~/data/ mcp
```

## Natural language setup (optional)

NL queries require [Ollama](https://ollama.com):

```sh
# Install Ollama
brew install ollama        # macOS
# or: https://ollama.com/download

# Pull the model (986 MB, runs on CPU)
ollama pull qwen2.5-coder:1.5b
```

SQL queries and the MCP server work without Ollama.

## Configure your AI tool

### RooCode / Cursor

```json
{
  "mcpServers": {
    "pipetable": {
      "command": "pipetable",
      "args": ["mcp"]
    }
  }
}
```

### Claude Code

```bash
claude mcp add pipetable pipetable mcp
```

### VS Code GitHub Copilot

```json
{
  "servers": {
    "pipetable": {
      "type": "stdio",
      "command": "pipetable",
      "args": ["mcp"]
    }
  }
}
```

## MCP tools

| Tool | Description |
|---|---|
| `scan_folder` | Register all data files in a folder (CSV, Parquet, JSON, TSV) |
| `list_datasets` | List registered datasets with column types |
| `get_schema` | Full schema + 3 sample rows for a dataset |
| `execute_sql` | Run SQL against registered datasets via DuckDB |

## License

Free for individuals and teams of up to 5 people.  
Commercial license required for organizations of 5+ people: https://pipetable.com/license
