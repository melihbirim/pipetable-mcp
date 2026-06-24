# Pipetable

Gives your AI coding tool real data access.

![Pipetable demo](https://github.com/melihbirim/pipetable/raw/main/demo/demo_nlq.gif)
 Point it at a folder of CSV, Parquet, JSON, or TSV files — your AI can now query them with real SQL instead of hallucinating.

Works as an MCP server for Claude Code, Cursor, RooCode, and Copilot. Also ships as a standalone CLI for interactive data exploration. Powered by DuckDB. Files never leave your machine.

**MIT licensed.**

## Install

```sh
# macOS / Linux
curl -fsSL https://pipetable.com/install | sh

# Windows
irm https://pipetable.com/install.ps1 | iex

# Rust
cargo install pipetable
```

## MCP server setup

### Claude Code
```sh
claude mcp add pipetable pipetable mcp
```

### Cursor / RooCode
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

### VS Code (Copilot)
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

Once configured, your AI can:
1. `scan_folder` — register all data files in a folder
2. `list_datasets` — see schemas and column types
3. `get_schema` — inspect a specific table with sample rows
4. `execute_sql` — run real DuckDB SQL against your files

Results are ground truth from DuckDB, not generated.

## CLI

```sh
pipetable ~/data/
```

SQL and natural language at the `>` prompt. SQL always works. Natural language requires [Ollama](https://ollama.com) running locally.

```
> SELECT region, SUM(revenue) AS total FROM sales GROUP BY 1 ORDER BY 2 DESC

4 row(s)

region  total
─────────────
EU      141000
US       32000
APAC     17000
```

```
> show me top 5 customers by revenue
Using: customers, sales
Thinking.....
SELECT c.name, SUM(s.revenue) AS total FROM customers c
JOIN sales s ON s.customer_id = c.id
GROUP BY c.name ORDER BY total DESC LIMIT 5
...
→ piped as _last
```

### Piping results

Every query saves its result as `_last` — a live DuckDB view you can query further:

```
> SELECT * FROM sales WHERE region = 'EU'
...
→ piped as _last

> show me top 3 from _last
Using: _last
Thinking.....
```

### Dot commands

| Command | Description |
|---|---|
| `.scan <path>` | Load a folder or file (Tab completes) |
| `.datasets` | List loaded datasets |
| `.schema <name>` | Columns + sample rows |
| `.drop <name>` | Remove a dataset from the session |
| `.use <n1> <n2>` | Focus NL queries on specific datasets |
| `.remove <name>` | Remove from focus |
| `.clear` | Reset focus to all datasets |
| `.model <name>` | Switch Ollama model |
| `.help` | Show help |

Tab completes dataset names after `FROM`, `JOIN`, `.schema`, `.drop`, `.use`.

### One-shot query

```sh
pipetable ask "who has the highest revenue?" ~/data/
pipetable ask "SELECT * FROM sales LIMIT 5" ~/data/
```

### Natural language (optional)

Set any one of these — pipetable auto-detects:

```sh
# Claude (best quality)
export ANTHROPIC_API_KEY=sk-ant-...

# OpenAI or any compatible API (LM Studio, Groq, Together, etc.)
export OPENAI_API_KEY=sk-...
export OPENAI_BASE_URL=http://localhost:1234  # optional, for local endpoints

# Ollama (local, no key needed)
ollama pull qwen2.5-coder:1.5b
ollama serve
```

Priority: Anthropic → OpenAI-compatible → Ollama. SQL and MCP work without any of them.

## Supported formats

CSV, Parquet, JSON, NDJSON, TSV, Excel (xlsx, xls, xlsm). Files up to 2GB. Folders scanned up to 3 levels deep. Hidden files and common noise directories (`node_modules`, `target`, `.git`) are skipped automatically.

## License

[MIT](LICENSE)
