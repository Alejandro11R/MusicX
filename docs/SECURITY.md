# Política de Seguridad y Protección de Cadena de Suministro (Supply Chain)

Este documento detalla las medidas de seguridad adoptadas en **Cadence** para proteger el código base, los binarios distribuidos y la máquina del mantenedor contra contribuciones o dependencias maliciosas.

---

## 1. Vectores de Riesgo en Proyectos Rust y TypeScript/Node

Al mantener un proyecto de código abierto, bajar y compilar un Pull Request (PR) de un tercero desconocido conlleva riesgos si no se toman precauciones:

1. **Scripts `build.rs` y Proc-Macros en Rust**:
   - `cargo build`, `cargo check` o incluso extensiones como `rust-analyzer` ejecutan código nativo arbitrario en la máquina local **antes** de compilar el binario final.
   - Si un PR añade un crate malicioso o modifica un `build.rs`, podría intentar acceder a variables de entorno, claves SSH (`~/.ssh`) o credenciales (`~/.cargo/credentials`).
2. **Scripts de ciclo de vida en npm (`preinstall`, `postinstall`, `prepare`)**:
   - Ejecutar `npm install` sin la bandera `--ignore-scripts` ejecuta código de Node con los mismos privilegios del usuario local.
3. **Permisos y Capacidades de Tauri (`tauri.conf.json` y `capabilities/`)**:
   - Un PR malicioso podría intentar relajar la Content Security Policy (CSP) o activar APIs de ejecución de comandos shell (`shell:allow-execute`) o acceso arbitrario al sistema de archivos.

---

## 2. Mecanismos de Protección en GitHub

### A. Branch Protection Rules (Rama `main`)
Para activar la máxima protección en el repositorio de GitHub:
1. Ir a **Settings > Branches > Branch protection rules** y añadir una regla para `main`.
2. **Require a pull request before merging**:
   - Activar **Require approvals** (mínimo 1 aprobación).
   - Activar **Dismiss stale pull request approvals when new commits are pushed**.
   - Activar **Require review from Code Owners** (basado en `.github/CODEOWNERS`).
3. **Require status checks to pass before merging**:
   - Seleccionar el check de CI (`Lint & SAST Checks`).
   - Activar **Require branches to be up to date before merging**.
4. **Do not allow bypassing the above settings**: Evita merges accidentales directos.

### B. Seguridad en GitHub Actions (`.github/workflows/ci.yml`)
- **Permisos estrictos de solo lectura**: `permissions: contents: read`.
- Los forks externos nunca tienen acceso a los secrets del repositorio ni a tokens de escritura.
- Las dependencias de Node se instalan con `npm ci --ignore-scripts` para neutralizar cualquier script oculto.
- La integración continua ejecuta `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, tests unitarios, tests de integración con `mpv` real (`--test integration -- --include-ignored --test-threads=1`) y compilación Tauri sin empaquetador (`npm run tauri build -- --no-bundle`).

### C. WebView CSP y Transporte Local IPC
- **Content Security Policy activa** (`src-tauri/tauri.conf.json`): restringe `default-src 'self'`, `script-src 'self'`, `object-src 'none'`, `base-uri 'self'`, `frame-ancestors 'none'`, permitiendo únicamente orígenes locales (`self`, `ipc:`, `http://ipc.localhost`, `asset:`, `http://asset.localhost`, `blob:`, `data:`) y miniaturas HTTPS de YouTube (`https://i.ytimg.com`, `https://*.ytimg.com`). Ver detalle en [`docs/LOCAL-TRANSPORT-STABILITY.md`](LOCAL-TRANSPORT-STABILITY.md).
- **Aislamiento de sockets Unix locales** (`src-tauri/src/services/local_transport.rs`, `control_server.rs`, `mpv_event.rs`, `bin/cli.rs`): los sockets de control y sesiones de `mpv` se crean dentro de directorios privados por usuario (`0700`, `cadence-<uid>`) con sockets `0600`, verificación anti-symlink/propietario, límite de encuadre de control de `16 KiB` (`MAX_CONTROL_LINE_BYTES`, CLI limitado a `64 KiB`) y apagado explícito del `Child` propietario sin archivos `.pid` globales ni escaneos de `/proc`.

---

## 3. Protocolo para Revisión Local Segura de Pull Requests

Si un colaborador externo envía un Pull Request y deseas probarlo en tu máquina:

### Regla de Oro: Inspeccionar el DIFF antes de compilar
**NUNCA** ejecutes `cargo run`, `cargo test` o `npm run dev` en una rama de un PR externo sin haber auditado primero los cambios:

```bash
# 1. Traer la rama del PR sin cambiar a ella
git fetch origin pull/<PR_NUMBER>/head:pr-<PR_NUMBER>

# 2. Revisar el diff exacto frente a main
git diff main...pr-<PR_NUMBER> --stat
git diff main...pr-<PR_NUMBER>
```

### Lista de Comprobación Crítica:
- [ ] ¿Se modificaron archivos `build.rs`, `.cargo/config.toml` o `tauri.conf.json`?
- [ ] ¿Se añadieron nuevas dependencias en `Cargo.toml` o `package.json`?
  - Verificar en [crates.io](https://crates.io) o [npmjs.com](https://npmjs.com) que el paquete no sea un typosquatting (ej. `serde-json` en vez de `serde_json`).
- [ ] ¿Hay scripts nuevos en `package.json` (`postinstall`, etc.)?
- [ ] ¿Se agregaron binarios, scripts `.sh` o archivos ofuscados en base64?

Si el PR supera la auditoría manual y el CI de GitHub pasa en verde, puedes probarlo localmente de forma segura.
