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

Type SQL or natural language at the `>` prompt. SQL runs directly; natural language requires [Ollama](https://ollama.com).

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

**Relevant tables are selected automatically.** When your folder has many unrelated files, pipetable scores each table by matching your question's keywords against table names and column names — only the best-matching tables are sent to the model.

```
> show me revenue by region       ← auto-selects the table with a "revenue" column
> how many products are inactive  ← auto-selects the products table
```

**Re-scanning is incremental** — only new or changed files are reloaded:

```
> .scan ~/data/
Found 47 files  (30×CSV  12×JSON  5×Parquet)
  ✓  new_export.csv   8 cols  2.3MB  45ms  new
  ✓  orders.csv      12 cols  4.1MB  32ms  updated

Done.  2 new  1 updated  44 unchanged
```

**Tab completion:**
- `.scan ~/Do` → completes filesystem paths
- `SELECT * FROM sal` → completes loaded dataset names
- `.schema ord` → completes dataset names
- `.sc` → completes dot commands

Dot commands:

| Command | Description |
|---|---|
| `.scan <path>` | Load a folder or file (Tab completes path) |
| `.datasets` | List loaded datasets |
| `.schema <name>` | Show columns + sample rows (Tab completes name) |
| `.models` | List available Ollama models |
| `.model <name>` | Switch model |
| `.help` | Show help |
| `.quit` | Exit |

### One-shot query

```sh
pipetable ask "who has the highest revenue?" ~/data/
pipetable ask "sales: show me revenue by region" ~/data/
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
