# Pipetable MCP Server

Query local CSV, Parquet, and JSON files from any AI coding tool (RooCode, Cursor, Claude Code, Copilot). Powered by DuckDB. Files never leave your machine.

## Install

**Option A — cargo install (requires Rust):**
```bash
cargo install --git https://github.com/melihbirim/pipetable-mcp
```

**Option B — download binary** from [Releases](https://github.com/melihbirim/pipetable-mcp/releases/latest):

| Platform | File |
|---|---|
| macOS (Apple Silicon + Intel) | `pipetable-mcp-macos-universal.tar.gz` |
| Linux x86_64 | `pipetable-mcp-linux-x86_64.tar.gz` |
| Linux ARM64 | `pipetable-mcp-linux-arm64.tar.gz` |
| Windows | `pipetable-mcp-windows-x86_64.zip` |

**macOS:**
```bash
tar xzf pipetable-mcp-macos-universal.tar.gz
chmod +x pipetable-mcp-universal
sudo mv pipetable-mcp-universal /usr/local/bin/pipetable-mcp
```

**Linux:**
```bash
tar xzf pipetable-mcp-linux-x86_64.tar.gz   # or linux-arm64
chmod +x pipetable-mcp-linux-x86_64
sudo mv pipetable-mcp-linux-x86_64 /usr/local/bin/pipetable-mcp
```

**Windows:** extract the zip, place `pipetable-mcp.exe` somewhere on your PATH.

## Configure your AI tool

### RooCode (VS Code)

Add to `.roo/mcp.json` in your project (or global settings):

```json
{
  "mcpServers": {
    "pipetable": {
      "command": "pipetable-mcp",
      "args": []
    }
  }
}
```

### Cursor

Add to `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "pipetable": {
      "command": "pipetable-mcp",
      "args": []
    }
  }
}
```

### Claude Code

```bash
claude mcp add pipetable pipetable-mcp
```

### VS Code GitHub Copilot

Add to `.vscode/mcp.json`:

```json
{
  "servers": {
    "pipetable": {
      "type": "stdio",
      "command": "pipetable-mcp",
      "args": []
    }
  }
}
```

## Tools

| Tool | Description |
|---|---|
| `scan_folder` | Register all data files in a folder (CSV, Parquet, JSON, TSV) |
| `list_datasets` | List registered datasets with column types |
| `get_schema` | Full schema + 3 sample rows for a dataset |
| `execute_sql` | Run SQL against registered datasets via DuckDB |

## Example usage

Ask your AI assistant:

> "Scan my ~/data folder and show me total revenue by region from sales.csv"

The assistant will call `scan_folder`, then `execute_sql` — results come from DuckDB, not hallucination.

## License

Free for individuals and teams of up to 5 people.  
Commercial license required for organizations of 5+ people: https://pipetable.com/license
