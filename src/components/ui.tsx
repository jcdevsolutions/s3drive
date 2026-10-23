import { useEffect, useRef } from "react";
import type { ReactNode } from "react";

export function Badge({
  tone = "neutral",
  children,
}: {
  tone?: "neutral" | "success" | "warning" | "danger" | "accent";
  children: ReactNode;
}) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}

export function Toggle({
  checked,
  onChange,
  label,
  description,
  disabled = false,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
}) {
  return (
    <label className={`toggle${disabled ? " toggle--disabled" : ""}`}>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="toggle__track" aria-hidden="true">
        <span className="toggle__thumb" />
      </span>
      <span className="toggle__text">
        <span className="toggle__label">{label}</span>
        {description ? <span className="toggle__description">{description}</span> : null}
      </span>
    </label>
  );
}

export function Field({
  label,
  hint,
  htmlFor,
  children,
}: {
  label: string;
  hint?: string;
  htmlFor?: string;
  children: ReactNode;
}) {
  return (
    <div className="field">
      <label className="field__label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {hint ? <p className="field__hint">{hint}</p> : null}
    </div>
  );
}

export function Modal({
  title,
  subtitle,
  onClose,
  children,
  footer,
  wide = false,
}: {
  title: string;
  subtitle?: string;
  onClose: () => void;
  children: ReactNode;
  footer: ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    ref.current?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="overlay" role="presentation" onMouseDown={onClose}>
      <div
        className={`modal${wide ? " modal--wide" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        ref={ref}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal__header">
          <div>
            <h2>{title}</h2>
            {subtitle ? <p className="modal__subtitle">{subtitle}</p> : null}
          </div>
          <button type="button" className="icon-button" onClick={onClose} aria-label="Close">
            ×
          </button>
        </header>
        <div className="modal__body">{children}</div>
        <footer className="modal__footer">{footer}</footer>
      </div>
    </div>
  );
}

export function ConfirmDialog({
  title,
  message,
  detail,
  confirmLabel,
  tone = "danger",
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  detail?: string;
  confirmLabel: string;
  tone?: "danger" | "accent";
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="overlay" role="presentation" onMouseDown={onCancel}>
      <div
        className="modal modal--narrow"
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal__header">
          <h2>{title}</h2>
        </header>
        <div className="modal__body">
          <p>{message}</p>
          {detail ? <pre className="code-block">{detail}</pre> : null}
        </div>
        <footer className="modal__footer">
          <button type="button" className="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className={`button button--${tone}`} onClick={onConfirm}>
            {confirmLabel}
          </button>
        </footer>
      </div>
    </div>
  );
}

export function LogPanel({
  lines,
  running,
  onClear,
  placeholder = "Log output appears here.",
}: {
  lines: string[];
  running: boolean;
  onClear: () => void;
  placeholder?: string;
}) {
  return (
    <div className="log">
      <div className="log__bar">
        <span className={`dot ${running ? "dot--live" : "dot--idle"}`} />
        <span className="log__status">{running ? "Running" : "Idle"}</span>
        <button type="button" className="link-button" onClick={onClear}>
          Clear log
        </button>
      </div>
      <pre className="log__body">
        {lines.length === 0 ? <span className="log__empty">{placeholder}</span> : lines.join("\n")}
      </pre>
    </div>
  );
}

export function EmptyState({
  title,
  message,
  action,
}: {
  title: string;
  message: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      <h3>{title}</h3>
      <p>{message}</p>
      {action}
    </div>
  );
}

export function StatusDot({ state }: { state: "ok" | "warn" | "off" }) {
  return <span className={`dot dot--${state}`} aria-hidden="true" />;
}

export function Banner({
  tone,
  title,
  children,
  action,
}: {
  tone: "info" | "warning" | "danger";
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className={`banner banner--${tone}`}>
      <div>
        <strong>{title}</strong>
        {children ? <div className="banner__body">{children}</div> : null}
      </div>
      {action}
    </div>
  );
}
