import { useId, useRef, useState } from "react";
import "./controls.css";

type ToggleProps = {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
};

export function Toggle({ checked, onChange, label, description, disabled }: ToggleProps) {
  const id = useId();
  return (
    <div className="field">
      <div className="field__text">
        <label htmlFor={id} className="field__label">
          {label}
        </label>
        {description ? <p className="field__hint">{description}</p> : null}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        className="switch"
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="switch__thumb" />
      </button>
    </div>
  );
}

type SliderProps = {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  unit?: string;
  format?: (value: number) => string;
  onChange: (next: number) => void;
};

export function Slider({ label, value, min, max, step = 1, unit = "", format, onChange }: SliderProps) {
  const id = useId();
  const [active, setActive] = useState(false);
  const ratio = (value - min) / (max - min);
  const shown = format ? format(value) : `${value}${unit}`;
  return (
    <div className="slider">
      <div className="slider__head">
        <label htmlFor={id}>{label}</label>
        <span className={active ? "slider__value slider__value--active" : "slider__value"}>{shown}</span>
      </div>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        style={{ ["--fill" as string]: `${ratio * 100}%` }}
        onPointerDown={() => setActive(true)}
        onPointerUp={() => setActive(false)}
        onBlur={() => setActive(false)}
        onFocus={() => setActive(true)}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </div>
  );
}

type SegmentedProps<T extends string> = {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (next: T) => void;
};

export function Segmented<T extends string>({ label, value, options, onChange }: SegmentedProps<T>) {
  const index = Math.max(0, options.findIndex((option) => option.value === value));
  return (
    <div className="segmented" role="group" aria-label={label}>
      <span
        className="segmented__indicator"
        style={{
          width: `calc((100% - 8px) / ${options.length})`,
          transform: `translateX(calc(${index} * 100%))`,
        }}
      />
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={option.value === value}
          className="segmented__item"
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

type ComboboxProps = {
  label: string;
  value: string;
  options: { value: string; label: string; native: string }[];
  onChange: (next: string) => void;
};

export function LanguageCombobox({ label, value, options, onChange }: ComboboxProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = options.find((option) => option.value === value);
  const matches = options.filter((option) =>
    `${option.label} ${option.native}`.toLowerCase().includes(query.trim().toLowerCase()),
  );

  return (
    <div
      className="combobox"
      ref={rootRef}
      onBlur={(event) => {
        if (!rootRef.current?.contains(event.relatedTarget as Node)) {
          setOpen(false);
        }
      }}
    >
      <span className="combobox__label">{label}</span>
      <button
        type="button"
        className="combobox__trigger"
        aria-expanded={open}
        aria-haspopup="listbox"
        onClick={() => setOpen((previous) => !previous)}
      >
        <span>{selected?.native ?? value}</span>
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path d="M4 6.5 8 10.5 12 6.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        </svg>
      </button>
      {open ? (
        <div className="combobox__panel" role="listbox">
          <input
            className="combobox__search"
            autoFocus
            value={query}
            placeholder="Find a language"
            onChange={(event) => setQuery(event.target.value)}
          />
          <div className="combobox__list">
            {matches.map((option) => (
              <button
                key={option.value}
                type="button"
                role="option"
                aria-selected={option.value === value}
                className="combobox__option"
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                  setQuery("");
                }}
              >
                <span>{option.native}</span>
                <span className="combobox__latin">{option.label}</span>
              </button>
            ))}
            {matches.length === 0 ? <p className="combobox__empty">No language matches “{query}”.</p> : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}

export function Card({
  title,
  description,
  children,
  action,
}: {
  title?: string;
  description?: string;
  action?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <section className="card">
      {title ? (
        <header className="card__head">
          <div>
            <h2 className="card__title">{title}</h2>
            {description ? <p className="card__desc">{description}</p> : null}
          </div>
          {action}
        </header>
      ) : null}
      {children}
    </section>
  );
}
