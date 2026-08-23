import type { Config } from "tailwindcss";
import tailwindcssAnimate from "tailwindcss-animate";

// Источник истины для токенов дизайн-системы (см. 06-DESIGN-SYSTEM.md).
// Компоненты обязаны ссылаться на семантические классы (bg-surface-1,
// text-primary, border-default...), а не на сырые шкалы (neutral-3) напрямую —
// это делает переключение темы (CSS custom properties в globals.css)
// бесплатным, без перекомпиляции Tailwind-классов.
export default {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        surface: {
          0: "var(--surface-0)", // фон приложения (за канвасом)
          1: "var(--surface-1)", // панели (Inspector, ActivityBar)
          2: "var(--surface-2)", // карточки узлов графа, поля ввода
          overlay: "var(--surface-overlay)", // Command Palette, модалки
        },
        border: {
          DEFAULT: "var(--border-default)",
          subtle: "var(--border-subtle)",
          focus: "var(--border-focus)",
        },
        text: {
          primary: "var(--text-primary)",
          secondary: "var(--text-secondary)",
          muted: "var(--text-muted)",
          inverted: "var(--text-inverted)",
        },
        accent: {
          DEFAULT: "var(--accent-9)",
          hover: "var(--accent-10)",
          subtle: "var(--accent-3)",
        },
        status: {
          idle: "var(--status-idle)",
          running: "var(--status-running)",
          error: "var(--status-error)",
          stale: "var(--status-stale)",
        },
      },
      fontFamily: {
        sans: ["Inter", "-apple-system", "BlinkMacSystemFont", "sans-serif"],
        mono: ["JetBrains Mono", "SFMono-Regular", "Menlo", "monospace"],
      },
      fontSize: {
        "2xs": ["11px", { lineHeight: "16px" }],
        xs: ["12px", { lineHeight: "16px" }],
        sm: ["13px", { lineHeight: "20px" }],
        base: ["14px", { lineHeight: "20px" }],
        lg: ["16px", { lineHeight: "24px" }],
        xl: ["20px", { lineHeight: "28px" }],
        "2xl": ["24px", { lineHeight: "32px" }],
      },
      spacing: {
        px: "1px",
        0.5: "2px",
        1: "4px",
        2: "8px",
        3: "12px",
        4: "16px",
        5: "20px",
        6: "24px",
        8: "32px",
        10: "40px",
        12: "48px",
        16: "64px",
      },
      borderRadius: {
        sm: "var(--radius-sm)",
        md: "var(--radius-md)",
        lg: "var(--radius-lg)",
      },
      boxShadow: {
        panel: "var(--shadow-panel)",
        overlay: "var(--shadow-overlay)",
      },
      transitionDuration: {
        fast: "150ms",
        base: "200ms",
      },
      transitionTimingFunction: {
        "out-expo": "cubic-bezier(0.16, 1, 0.3, 1)",
      },
      backgroundImage: {
        "graph-grid":
          "radial-gradient(circle, var(--grid-dot) 1px, transparent 1px)",
      },
      backgroundSize: {
        "graph-grid": "24px 24px",
      },
    },
  },
  plugins: [tailwindcssAnimate],
} satisfies Config;
