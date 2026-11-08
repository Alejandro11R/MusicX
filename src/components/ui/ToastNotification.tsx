"use client";

import { CheckCircle2, RotateCcw, X } from "lucide-react";

export interface ToastMessage {
  id: string;
  text: string;
  onUndo?: () => void;
}

interface ToastNotificationProps {
  toast: ToastMessage | null;
  onClose: () => void;
}

export function ToastNotification({ toast, onClose }: ToastNotificationProps) {
  if (!toast) return null;

  return (
    <div
      role="status"
      aria-live="polite"
      className="fixed bottom-20 right-6 z-50 flex items-center gap-3 px-4 py-2.5 rounded-2xl bg-neutral-900/95 border border-white/20 text-white shadow-2xl backdrop-blur-2xl animate-in fade-in slide-in-from-bottom-3 duration-150 max-w-sm"
    >
      <CheckCircle2 className="size-4 text-teal-300 shrink-0" />
      <span className="text-xs font-medium truncate flex-1">{toast.text}</span>

      {toast.onUndo && (
        <button
          type="button"
          onClick={() => {
            toast.onUndo?.();
            onClose();
          }}
          className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-teal-500/20 hover:bg-teal-500/30 text-teal-200 border border-teal-400/30 text-[11px] font-semibold transition-all cursor-pointer shrink-0 active:scale-95"
        >
          <RotateCcw className="size-3" />
          <span>Deshacer</span>
        </button>
      )}

      <button
        type="button"
        onClick={onClose}
        className="p-1 rounded-full text-white/40 hover:text-white"
        aria-label="Cerrar notificación"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
}

export default ToastNotification;
