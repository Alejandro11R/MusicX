# Plan de Refactorización - Cadence

## Fase 0: Preparación y Tests Base
- [x] Ejecutar tests existentes para establecer baseline
- [x] Añadir tests de integración críticos si faltan
- [x] Configurar tracing/logging estructurado

## Fase 1: Concurrencia Core (P0) - Backend Rust
- [x] **1.1** AppState con locks granulares (playback, track, heavy_ops, mpv_status)
- [x] **1.2** MpvPlayer event-driven: task dedicado + broadcast/watch channels
- [x] **1.3** PlaybackController (control rápido sin yt-dlp)
- [x] **1.4** TrackResolver con cache + semáforo + circuit breaker
- [x] **1.5** PlayerOrchestrator + StateMachine
- [x] **1.6** Comandos Tauri actualizados para nueva API
- [x] **1.7** Tests de concurrencia: play/pause/seek simultáneos

## Fase 2: Frontend Event-Driven (P0-C)
- [x] **2.1** Tauri events para player-state-changed, playback-ended, etc.
- [x] **2.2** Eliminar polling 250ms → event listeners + fallback 5s
- [x] **2.3** AbortController para cancelación de requests
- [x] **2.4** React.memo + separación de componentes (SearchResults, NowPlaying, VolumeControl)

## Fase 3: Observabilidad y Errores (P1)
- [x] **3.1** tracing + tracing-subscriber (JSON, env-filter)
- [x] **3.2** FrontendError enum tipado + serialización
- [x] **3.3** Instrumentación en servicios críticos

## Fase 4: yt-dlp Hardening (P2-G)
- [x] **4.1** Cache con TTL (30 min) + validación HEAD
- [x] **4.2** Semáforo (max 2 concurrent) + Circuit breaker (5 fallos → 60s reset)
- [x] **4.3** Search cache + audio URL cache

## Fase 5: Validación y Benchmarks
- [x] **5.1** Tests backend: 39 passed, 3 ignored (`cargo test -- --test-threads=1`)
- [x] **5.2** Frontend: 15 tests de `src/lib/playback.ts` (`npm test`) y build OK (`npm run build`)
- [x] **5.3** Backend build (dev): OK (`cargo check`, sin warnings)
- [ ] **5.4** Verificar funcionalidad completa manualmente (requiere mpv + yt-dlp)

> Nota: los tests que resuelven audio real (`services::player::*`,
> `yt_dlp::resolve_audio_*`, `yt_dlp::search_*`) dependen de la red. En
> paralelo saturan YouTube y pueden fallar por timeout; correr con
> `cargo test -- --test-threads=1` para una suite determinista.

## Fase 6: Resiliencia y Controles (auditoría)
- [x] **6.1** Supervisor de mpv con reinicio automático y backoff 250ms → 5s
- [x] **6.2** `observe_property` (pause, volume, time-pos, duration, loop-file) para estado real
- [x] **6.3** Reconexión transparente: re-resolver, recargar y restaurar posición/volumen/loop/pausa
- [x] **6.4** URLs firmadas: TTL derivado de `expire` (margen 60s, tope 30 min)
- [x] **6.5** Reintento acotado (2) con invalidación de caché ante 403/stream caído
- [x] **6.6** `end-file` con reason `stop`/`quit` ignorado (evita borrar el track recién cargado)
- [x] **6.7** Evento `Progress` a 2 Hz + `ConnectionChanged` para el banner de reconexión
- [x] **6.8** Frontend: seek con preview/commit, guard anti-rebote, volumen preview/commit, loop/pausa optimistas
- [x] **6.9** Estado `connected` en `PlayerState` (compatible con payloads antiguos)

## Fase 7: Verificación de Integración y Debugging
- [x] **7.1** Tests end-to-end con mpv real + yt-dlp stub (sin red): `src-tauri/tests/integration.rs`
- [x] **7.2** Smoke test de la app real (arranque, SIGTERM, salida limpia)
- [x] **7.3** Limpieza de mpv huérfanos entre ejecuciones

### Bugs reales encontrados y corregidos en la verificación

1. **`end-file` ilegible con mpv ≥ 0.40** (`mpv_event.rs`)
   mpv 0.41 envía `{"event":"end-file","reason":"error","file_error":"loading failed"}`,
   con el detalle en el nivel del evento; el código solo miraba dentro de `data`
   (formato de mpv viejo). Consecuencia: `reason` siempre `"unknown"` y la
   **recuperación de URLs caducadas jamás se habría disparado**. Se leen ambos
   formatos, con tests basados en JSON capturados del mpv real.

2. **Carrera en la actualización optimista** (`mpv_event.rs`)
   El estado se aplicaba al enviar el comando, de modo que un `property-change`
   atrasado de un comando anterior podía pisar al más reciente (observable con
   `set_loop(true)` + `set_loop(false)` seguidos). Ahora se aplica al confirmar
   la respuesta de mpv, que ya ha emitido sus `property-change`.

3. **mpv huérfano al salir** (`mpv_event.rs`, `lib.rs`)
   Con SIGTERM (o un `tauri dev` que recompila) la app moría sin apagar a mpv:
   se acumulaba un proceso por arranque. Ahora hay (a) manejador de SIGTERM que
   sale por el camino limpio, (b) `shutdown()` que espera a que el supervisor
   mate al hijo, y (c) un pidfile junto al socket que mata al mpv de una
   ejecución anterior si la app murió sin shutdown (solo si su cmdline contiene
   nuestro socket).

4. **Tests de mpv sin apagado**
   Los tests ignorados de `mpv_event` dejaban mpv corriendo tras cada ejecución.

### Cómo reproducir la verificación
```bash
# Backend completo (unitarios + integración con mpv real, sin red):
cd src-tauri && cargo test -- --include-ignored --test-threads=1
# Solo los tests end-to-end:
cargo test --test integration -- --include-ignored --test-threads=1
# Frontend:
cd .. && npm test && npm run build
```

---

## Resumen de Cambios Implementados

### Backend (Rust)

#### Nueva Arquitectura de Concurrencia
| Antes | Después |
|-------|---------|
| `Arc<Mutex<PlayerService>>` - Un lock para todo | `PlayerOrchestrator` - Locks granulares + channels |
| `state()` = 4 IPC round-trips secuenciales | `status()` = lectura instantánea watch channel (0 IPC) |
| Polling 250ms (16 IPC/s) | Event-driven (0 IPC/s en idle) |
| yt-dlp bloquea todo el mutex | Semáforo (2 concurrent) + Circuit breaker |

#### Módulos Nuevos/Refactorizados
- `services/mpv_event.rs` - MpvPlayer event-driven con task dedicado, broadcast/watch channels
- `services/orchestrator.rs` - PlayerOrchestrator, PlaybackController, TrackResolver, StateMachine
- `services/yt_dlp.rs` - Cache TTL, semáforo, circuit breaker, search cache
- `errors.rs` - FrontendError tipado para manejo granular en UI

#### Métricas de Mejora Esperadas
| Métrica | Baseline | Target |
|---------|----------|--------|
| `play()` latency (post-resolve) | ~200-500ms | <50ms |
| `seek()` latency | ~100-300ms | <20ms |
| `state()` IPC calls/sec | 16 (polling) | 0 (events) |
| Mutex contention | High | Zero |
| Cold start (cached) | ~5-10s | <3s |

### Frontend (React/TypeScript)

#### Cambios Principales
1. **Event-driven state sync**: `listenOrchestratorEvents` reemplaza polling 250ms
2. **AbortController**: Cancelación de búsquedas previas al iniciar nueva
3. **Componentes memoizados**: `SearchResultItem`, `RecentTrackItem`, `NowPlaying`
4. **Tipado estricto**: `OrchestratorEvent` union type para eventos del backend

#### Archivos Actualizados
- `src/lib/tauri.ts` - Nuevas funciones `subscribeEvents`, `listenOrchestratorEvents`, tipos `OrchestratorEvent`
- `src/lib/playback.ts` - Utilidades puras de playback (`clamp`, `safeDuration`, `safePosition`, `seekTarget`, `formatDuration`)
- `src/App.tsx` - Arquitectura event-driven, componentes separados, AbortController

#### Controles de reproducción (auditoría)
- **Loop**: estado optimista + `desired_loop` en el orquestador; se re-aplica tras cada carga/reconexión.
- **Progreso**: el backend emite `Progress` a 2 Hz; el slider hace preview al arrastrar y `seek` al soltar.
  Un guard de 1.2s evita que el thumb rebote con posiciones viejas de mpv.
- **Volumen**: preview local al arrastrar y commit al soltar (evita una tormenta de IPC).
- **Teclado**: los atajos leen `playerStateRef`, así que no se quedan con estado obsoleto.
- **Reconexión**: banner ámbar cuando `connected === false`; al volver se refresca el estado completo.

---

## Checklist de No-Regresión Funcional

### Playback Básico
- [ ] Play track → suena audio
- [ ] Pause → pausa
- [ ] Resume → continua
- [ ] Stop → detiene y limpia current
- [ ] Seek → salta a posición
- [ ] Volume → cambia volumen
- [ ] Loop → toggle loop

### Búsqueda
- [ ] Search "query" → resultados
- [ ] Click resultado → play
- [ ] Enter en search → play primer resultado
- [ ] Escape → limpia búsqueda

### Historial
- [ ] Recent tracks persisten en localStorage
- [ ] Click recent → play
- [ ] Clear recent → limpia

### UI/UX
- [ ] Keyboard shortcuts: Space, M, L, ArrowLeft/Right, /, Cmd+K, Cmd+Q
- [ ] Tray icon: toggle show/hide, quit
- [ ] Window: minimize, hide (no close), drag region
- [ ] Loading states visibles
- [ ] Error messages dismissibles

### Salud
- [ ] Health check mpv/yt-dlp al inicio
- [ ] Shutdown limpio (mpv muere con app)

---

## Próximos Pasos Recomendados

1. **Testing manual**: Ejecutar `cargo tauri dev` y verificar funcionalidad completa
2. **Benchmarks**: Medir latencias reales con mpv + yt-dlp
3. **Release build**: `cargo tauri build` para distribución
4. **Monitoreo**: Verificar logs JSON estructurados en producción
5. **Futuro**: Playlist/queue, MPRIS completo, archivos locales, visualización audio