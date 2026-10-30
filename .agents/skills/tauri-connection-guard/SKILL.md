---
name: tauri-connection-guard
description: Previene y resuelve el error 'Could not connect to localhost: Connection refused' en Tauri verificando compilación de producción limpia y gestión de procesos.
---

# Tauri Connection Guard

Guía concisa para evitar que la aplicación intente conectarse a `localhost:1420` en lugar de cargar los recursos empaquetados.

## 1. Causa Raíz

El error **"Could not connect to localhost: Connection refused"** en Linux ocurre cuando:
1. **Compilación incorrecta con Cargo crudo:** Ejecutar `cargo build --release` directamente en `src-tauri` no activa los hooks de Tauri CLI. El binario asume modo de desarrollo y busca `http://localhost:1420`.
2. **Proceso zombie en ejecución:** Un binario compilado anteriormente sigue activo en segundo plano y el sistema operativo no carga el nuevo.

---

## 2. Regla de Compilación

| Acción | Comando | Motivo |
|---|---|---|
| ❌ **Prohibido** | `cargo build --release` | Omite empaquetado de assets; busca Vite en `localhost:1420`. |
| ✅ **Correcto** | `npm run tauri build -- --no-bundle` | Compila frontend (`dist`) y lo incrusta en el binario nativo. |

---

## 3. Protocolo de Verificación (3 Pasos)

Cuando se actualice el código o se prepare una versión para el usuario:

```bash
# 1. Terminar procesos viejos colgados
pkill -f "target/release/cadence" 2>/dev/null || true

# 2. Compilar binario de producción embebido
npm run tauri build -- --no-bundle

# 3. Validar arranque sin errores de conexión
timeout 3s ./src-tauri/target/release/cadence || true
```

---

## 4. Modo Desarrollo (`tauri dev`)

Si se trabaja en desarrollo:
* Asegurarse de que el puerto 1420 esté libre antes de iniciar:
  ```bash
  lsof -ti:1420 | xargs kill -9 2>/dev/null || true
  npm run tauri dev
  ```
