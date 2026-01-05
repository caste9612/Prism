# Development Guide

## Prerequisites

- **Node.js** 18+ - https://nodejs.org/
- **Rust** 1.70+ - https://rustup.rs/
- **Visual Studio Build Tools** (Windows) - Required for Rust compilation

## Setup

```bash
# Clone repository
git clone https://github.com/user/prism.git
cd prism

# Install Node dependencies
npm install

# Run in development mode
npm run tauri dev
```

## Project Commands

| Command | Description |
|---------|-------------|
| `npm run dev` | Start Vite dev server (frontend only) |
| `npm run build` | Build frontend for production |
| `npm run check` | Run Svelte type checking |
| `npm run tauri dev` | Run full Tauri app in dev mode |
| `npm run tauri build` | Build production release |

## Rust Commands

```bash
cd src-tauri

# Check compilation
cargo check

# Run tests
cargo test

# Format code
cargo fmt

# Lint
cargo clippy
```

## Code Style

### TypeScript/Svelte

- Use TypeScript strict mode
- Prefer reactive statements (`$:`) over imperative updates
- Use stores for shared state
- Format with Prettier (via editor)

### Rust

- Follow Rust 2021 edition idioms
- Use `thiserror` for error types
- Use `tracing` for logging
- Format with `cargo fmt`
- Lint with `cargo clippy`

## Adding a New Feature

### 1. Backend Command

```rust
// src-tauri/src/commands/mod.rs

#[tauri::command]
pub async fn my_new_command(
    state: State<'_, AppState>,
    param: String,
) -> Result<MyResponse, String> {
    let db = state.db.lock().await;
    // ... implementation
    Ok(result)
}
```

Register in `src-tauri/src/lib.rs`:
```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands
    commands::my_new_command,
])
```

### 2. Frontend Store

```typescript
// src/lib/stores/myfeature.ts

import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

interface MyState {
    data: MyData[];
    loading: boolean;
    error: string | null;
}

function createMyStore() {
    const { subscribe, set, update } = writable<MyState>({
        data: [],
        loading: false,
        error: null,
    });

    return {
        subscribe,
        load: async () => {
            update(s => ({ ...s, loading: true }));
            try {
                const data = await invoke<MyData[]>('my_new_command');
                update(s => ({ ...s, data, loading: false }));
            } catch (e) {
                update(s => ({ ...s, loading: false, error: String(e) }));
            }
        },
    };
}

export const myStore = createMyStore();
```

### 3. UI Component

```svelte
<script lang="ts">
    import { myStore } from '$lib/stores/myfeature';

    $: data = $myStore.data;
    $: loading = $myStore.loading;
</script>

{#if loading}
    <div>Loading...</div>
{:else}
    {#each data as item}
        <div>{item.name}</div>
    {/each}
{/if}
```

## Debugging

### Frontend

- Use browser DevTools (F12 in Tauri window)
- Console logs appear in DevTools
- React DevTools works with Svelte via extension

### Backend

Enable debug logging:
```bash
RUST_LOG=prism=debug npm run tauri dev
```

Logs appear in terminal.

### Database

SQLite database location:
- Windows: `%APPDATA%\com.prism.diskanalyzer\prism.db`

Inspect with any SQLite viewer (e.g., DB Browser for SQLite).

## Testing

### Frontend

```bash
# Type checking
npm run check

# (Future) Unit tests
npm test
```

### Backend

```bash
cd src-tauri
cargo test
```

## Build Artifacts

Production build outputs:
```
src-tauri/target/release/
├── prism.exe              # Portable executable
└── bundle/
    ├── msi/               # MSI installer
    └── nsis/              # NSIS installer
```

## Troubleshooting

### Build fails with linker errors
Ensure Visual Studio Build Tools are installed with C++ workload.

### Frontend not loading
Check port 1420 is not in use.

### Icons missing
Generate icons:
```bash
npm run tauri icon path/to/icon.png
```

### Database locked
Close any SQLite viewers accessing the database.
