"use client"

import React, { type HTMLProps } from "react"
import { cn } from "@/lib/utils"

/* ============================================================
 * GlassPanel — contenedor de vidrio con brillo especular.
 * Úsalo para tarjetas flotantes, paneles laterales, now-playing.
 * ============================================================ */
interface GlassPanelProps extends HTMLProps<HTMLDivElement> {
  pressed?: boolean
  children: React.ReactNode
}

export function GlassPanel({
  className,
  pressed,
  children,
  onPointerMove,
  ...props
}: GlassPanelProps) {
  const panelRef = React.useRef<HTMLDivElement>(null)
  const rafRef = React.useRef<number | null>(null)

  React.useEffect(() => {
    return () => {
      if (rafRef.current !== null) {
        window.cancelAnimationFrame(rafRef.current)
      }
    }
  }, [])

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    onPointerMove?.(e)
    const el = panelRef.current
    if (!el || rafRef.current !== null) return
    const clientX = e.clientX
    const clientY = e.clientY
    rafRef.current = window.requestAnimationFrame(() => {
      rafRef.current = null
      const rect = el.getBoundingClientRect()
      el.style.setProperty("--pointer-x", `${Math.round(clientX - rect.left)}px`)
      el.style.setProperty("--pointer-y", `${Math.round(clientY - rect.top)}px`)
    })
  }

  return (
    <div
      ref={panelRef}
      data-pressed={pressed ? true : undefined}
      className={cn("glass-panel rounded-3xl p-4", className)}
      onPointerMove={handlePointerMove}
      {...props}
    >
      {children}
    </div>
  )
}

/* ============================================================
 * GlassButton — botón translúcido con respuesta física.
 * Variantes: ghost (secundario) | primary (acción principal).
 * ============================================================ */
interface GlassButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "size"> {
  variant?: "ghost" | "primary"
  size?: "sm" | "md" | "lg"
}

export function GlassButton({
  className,
  variant = "ghost",
  size = "md",
  children,
  ...props
}: GlassButtonProps) {
  const sizeMap = {
    sm: "size-7 text-xs",
    md: "size-9 text-sm",
    lg: "size-11 text-base",
  }
  const variantMap = {
    ghost: "glass-btn",
    primary: "glass-btn glass-btn-primary rounded-full text-xs font-semibold",
  }

  return (
    <button
      type="button"
      className={cn(
        "glass-btn rounded-xl",
        variantMap[variant],
        sizeMap[size],
        className,
      )}
      {...props}
    >
      {children}
    </button>
  )
}

/* ============================================================
 * GlassSlider — slider de vidrio con rango luminoso.
 * Nota: es un componente de presentación simple. Para el
 * player real se recomienda seguir usando Radix Slider y
 * aplicar las clases `.glass-slider-*` del CSS.
 * ============================================================ */
interface GlassSliderProps {
  value: number
  max: number
  onChange: (value: number) => void
  className?: string
}

export function GlassSlider({
  value,
  max,
  onChange,
  className,
}: GlassSliderProps) {
  const pct = Math.max(0, Math.min(100, (value / Math.max(max, 1)) * 100))
  const [active, setActive] = React.useState(false)

  return (
    <div className={cn("w-full h-2", className)}>
      <div className="glass-slider-track w-full cursor-pointer">
        <div
          className="glass-slider-range"
          style={{ width: `${pct}%` }}
        />
        <div
          className="absolute -translate-x-1/2 top-1/2 -translate-y-1/2 glass-slider-thumb"
          style={{ left: `${pct}%` }}
          data-active={active ? true : undefined}
          onPointerDown={() => setActive(true)}
          onPointerUp={() => setActive(false)}
          onPointerLeave={() => setActive(false)}
          role="slider"
          aria-valuenow={value}
          aria-valuemin={0}
          aria-valuemax={max}
          onClick={(e) => {
            const rect = e.currentTarget.getBoundingClientRect()
            const clickX = e.clientX - rect.left
            const width = rect.width
            const next = Math.round((clickX / width) * max)
            onChange(next)
          }}
        />
      </div>
      <div className="flex justify-between mt-1 text-[10px] text-white/50 tabular-nums">
        <span>{Math.round(value)}</span>
        <span>{max}</span>
      </div>
    </div>
  )
}

/* ============================================================
 * MeshBackground — fondo de gradientes animado (depth).
 * Colócalo detrás de la ventana principal con z-index -1.
 * ============================================================ */
interface MeshBackgroundProps {
  playing?: boolean
  palette?: { primary: string; secondary: string; ambient: string }
}

export function MeshBackground({ playing, palette }: MeshBackgroundProps) {
  const accent = playing
    ? (palette?.primary ?? "rgba(45, 212, 191, 0.25)")
    : "rgba(99, 102, 241, 0.15)"
  const accent2 = playing
    ? (palette?.secondary ?? "rgba(16, 185, 129, 0.2)")
    : "rgba(139, 92, 246, 0.12)"
  const accent3 = playing
    ? (palette?.ambient ?? "rgba(255, 255, 255, 0.06)")
    : "rgba(255, 255, 255, 0.06)"

  return (
    <div className="mesh-bg">
      <div
        className="mesh-blob"
        style={{
          width: "60vw",
          height: "60vw",
          top: "-10%",
          left: "-10%",
          background: accent,
          animationDelay: "0s",
          transition: "background 900ms ease",
        }}
      />
      <div
        className="mesh-blob"
        style={{
          width: "55vw",
          height: "55vw",
          bottom: "-5%",
          right: "-5%",
          background: accent2,
          animationDelay: "-4s",
          transition: "background 900ms ease",
        }}
      />
      <div
        className="mesh-blob"
        style={{
          width: "45vw",
          height: "45vw",
          top: "25%",
          left: "55%",
          background: accent3,
          animationDelay: "-8s",
          transition: "background 900ms ease",
        }}
      />
    </div>
  )
}

/* ============================================================
 * SpecularRing — anillo de luz giratorio decorativo.
 * Úsalo alrededor del arte del álbum o del indicador de estado.
 * ============================================================ */
interface SpecularRingProps extends HTMLProps<HTMLDivElement> {
  children: React.ReactNode
}

export function SpecularRing({
  className,
  children,
  ...props
}: SpecularRingProps) {
  return (
    <div className={cn("specular-ring flex items-center justify-center", className)} {...props}>
      {children}
    </div>
  )
}
