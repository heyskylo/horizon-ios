"use client";

import styles from "./page.module.css";

interface Props<T extends string> {
  label: string;
  options: readonly { id: T; text: string }[];
  value: T;
  onChange: (id: T) => void;
}

export function Segmented<T extends string>({ label, options, value, onChange }: Props<T>) {
  return (
    <div className={styles.segmented} role="group" aria-label={label}>
      {options.map(({ id, text }) => (
        <button
          key={id}
          type="button"
          className={id === value ? styles.segmentActive : styles.segment}
          aria-pressed={id === value}
          onClick={() => onChange(id)}
        >
          {text}
        </button>
      ))}
    </div>
  );
}
