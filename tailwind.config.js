import plugin from 'tailwindcss/plugin';

const qb = {
  green: '#2ca01c',
  greenHover: '#248a17',
  greenActive: '#1d7313',
  greenTint: '#e9f6e7',
  nav: '#232733',
  navHover: '#2f3443',
  navActive: '#393f50',
  navText: '#babec5',
  canvas: '#f4f5f8',
  // The work area behind windows open inside LocalBooks, as the grey behind QuickBooks' windows.
  desktop: '#d9dde3',
  surface: '#ffffff',
  ink: '#393a3d',
  inkMuted: '#6b6c72',
  inkFaint: '#8d9096',
  border: '#d4d7dc',
  borderStrong: '#babec5',
  rowHover: '#f5f5f5',
  rowSelected: '#eef7ec',
  headerBg: '#eceef1',
  negative: '#d52b1e',
  warning: '#ff8000',
  info: '#0077c5',
};

/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,jsx,ts,tsx}'],
  theme: {
    extend: {
      colors: {
        qb: {
          green: {
            DEFAULT: qb.green,
            hover: qb.greenHover,
            active: qb.greenActive,
            tint: qb.greenTint,
          },
          nav: {
            DEFAULT: qb.nav,
            hover: qb.navHover,
            active: qb.navActive,
            text: qb.navText,
          },
          canvas: qb.canvas,
          desktop: qb.desktop,
          surface: qb.surface,
          ink: {
            DEFAULT: qb.ink,
            muted: qb.inkMuted,
            faint: qb.inkFaint,
          },
          border: {
            DEFAULT: qb.border,
            strong: qb.borderStrong,
          },
          row: {
            hover: qb.rowHover,
            selected: qb.rowSelected,
            header: qb.headerBg,
          },
          negative: qb.negative,
          warning: qb.warning,
          info: qb.info,
        },
      },
      fontFamily: {
        sans: [
          '-apple-system',
          'BlinkMacSystemFont',
          '"Segoe UI"',
          '"Helvetica Neue"',
          'Helvetica',
          'Arial',
          'sans-serif',
        ],
        mono: ['"SF Mono"', 'Menlo', 'Consolas', '"Liberation Mono"', 'monospace'],
      },
      // Dense financial type scale. 13px is the grid/body baseline, matching QuickBooks registers.
      fontSize: {
        '2xs': ['10px', { lineHeight: '14px' }],
        xs: ['11px', { lineHeight: '15px' }],
        sm: ['12px', { lineHeight: '16px' }],
        base: ['13px', { lineHeight: '18px' }],
        md: ['14px', { lineHeight: '20px' }],
        lg: ['16px', { lineHeight: '22px' }],
        xl: ['18px', { lineHeight: '24px' }],
        '2xl': ['22px', { lineHeight: '28px' }],
        '3xl': ['28px', { lineHeight: '34px' }],
      },
      spacing: {
        px: '1px',
        0.25: '1px',
        0.75: '3px',
        1.25: '5px',
        4.5: '18px',
        5.5: '22px',
        6.5: '26px',
        'row-xs': '20px',
        row: '24px',
        'row-lg': '28px',
        input: '26px',
        toolbar: '36px',
        sidebar: '200px',
        'sidebar-collapsed': '48px',
      },
      height: {
        'row-xs': '20px',
        row: '24px',
        'row-lg': '28px',
        input: '26px',
        toolbar: '36px',
      },
      minHeight: {
        row: '24px',
        input: '26px',
      },
      borderRadius: {
        none: '0',
        xs: '2px',
        sm: '3px',
        DEFAULT: '4px',
        md: '4px',
        lg: '6px',
      },
      boxShadow: {
        focus: `0 0 0 2px ${qb.green}40`,
        panel: '0 1px 2px rgba(0, 0, 0, 0.08)',
        popover: '0 4px 12px rgba(0, 0, 0, 0.16)',
        'row-active': `inset 3px 0 0 ${qb.green}`,
      },
      transitionDuration: {
        0: '0ms',
        75: '75ms',
      },
    },
  },
  plugins: [
    plugin(({ addBase, addComponents, addUtilities, theme }) => {
      addBase({
        html: {
          fontSize: theme('fontSize.base[0]'),
          lineHeight: theme('fontSize.base[1].lineHeight'),
          color: qb.ink,
          backgroundColor: qb.canvas,
          WebkitFontSmoothing: 'antialiased',
          MozOsxFontSmoothing: 'grayscale',
        },
        'input, select, textarea, button': {
          fontSize: 'inherit',
          lineHeight: 'inherit',
          color: 'inherit',
        },
      });

      addComponents({
        // Ledger / register grid
        '.qb-grid': {
          width: '100%',
          borderCollapse: 'collapse',
          tableLayout: 'fixed',
          backgroundColor: qb.surface,
          fontSize: theme('fontSize.base[0]'),
        },
        '.qb-grid-head': {
          height: theme('height.row'),
          padding: '0 8px',
          backgroundColor: qb.headerBg,
          color: qb.inkMuted,
          fontSize: theme('fontSize.xs[0]'),
          fontWeight: '600',
          textTransform: 'uppercase',
          letterSpacing: '0.02em',
          textAlign: 'left',
          whiteSpace: 'nowrap',
          borderBottom: `1px solid ${qb.borderStrong}`,
          position: 'sticky',
          top: '0',
          zIndex: '1',
        },
        '.qb-grid-row': {
          height: theme('height.row'),
          borderBottom: `1px solid ${qb.border}`,
          cursor: 'default',
          '&:hover': { backgroundColor: qb.rowHover },
          '&[aria-selected="true"], &.is-active': {
            backgroundColor: qb.rowSelected,
            boxShadow: theme('boxShadow.row-active'),
          },
        },
        '.qb-grid-cell': {
          height: theme('height.row'),
          padding: '0 8px',
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          verticalAlign: 'middle',
        },
        // Cell that hosts an inline editor: the input fills the cell edge-to-edge so activation causes zero layout shift.
        '.qb-grid-cell-edit': {
          height: theme('height.row'),
          padding: '0',
          verticalAlign: 'middle',
          '& > input, & > select': {
            width: '100%',
            height: '100%',
            padding: '0 7px',
            border: `1px solid ${qb.green}`,
            borderRadius: '0',
            backgroundColor: qb.surface,
            outline: 'none',
          },
        },

        // Form controls
        '.qb-input': {
          height: theme('height.input'),
          padding: '0 6px',
          backgroundColor: qb.surface,
          border: `1px solid ${qb.border}`,
          borderRadius: theme('borderRadius.xs'),
          outline: 'none',
          transition: 'border-color 75ms, box-shadow 75ms',
          '&:hover': { borderColor: qb.borderStrong },
          '&:focus': { borderColor: qb.green, boxShadow: theme('boxShadow.focus') },
          '&:disabled': { backgroundColor: qb.canvas, color: qb.inkFaint, cursor: 'not-allowed' },
          '&[aria-invalid="true"]': { borderColor: qb.negative },
        },
        '.qb-label': {
          display: 'block',
          marginBottom: '2px',
          color: qb.inkMuted,
          fontSize: theme('fontSize.xs[0]'),
          fontWeight: '600',
        },

        // Buttons
        '.qb-btn': {
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          gap: '4px',
          height: theme('height.input'),
          padding: '0 10px',
          fontSize: theme('fontSize.sm[0]'),
          fontWeight: '600',
          borderRadius: theme('borderRadius.xs'),
          border: '1px solid transparent',
          whiteSpace: 'nowrap',
          userSelect: 'none',
          '&:focus-visible': { outline: 'none', boxShadow: theme('boxShadow.focus') },
          '&:disabled': { opacity: '0.5', cursor: 'not-allowed' },
        },
        '.qb-btn-primary': {
          backgroundColor: qb.green,
          color: '#ffffff',
          '&:hover:not(:disabled)': { backgroundColor: qb.greenHover },
          '&:active:not(:disabled)': { backgroundColor: qb.greenActive },
        },
        '.qb-btn-secondary': {
          backgroundColor: qb.surface,
          color: qb.ink,
          borderColor: qb.borderStrong,
          '&:hover:not(:disabled)': { backgroundColor: qb.rowHover },
        },

        // Shell
        '.qb-sidebar': {
          width: theme('spacing.sidebar'),
          backgroundColor: qb.nav,
          color: qb.navText,
          fontSize: theme('fontSize.sm[0]'),
        },
        '.qb-nav-item': {
          display: 'flex',
          alignItems: 'center',
          gap: '8px',
          height: theme('height.row-lg'),
          padding: '0 12px',
          borderLeft: '3px solid transparent',
          cursor: 'pointer',
          '&:hover': { backgroundColor: qb.navHover, color: '#ffffff' },
          '&[aria-current="page"], &.is-active': {
            backgroundColor: qb.navActive,
            borderLeftColor: qb.green,
            color: '#ffffff',
            fontWeight: '600',
          },
        },
        '.qb-toolbar': {
          display: 'flex',
          alignItems: 'center',
          gap: '6px',
          height: theme('height.toolbar'),
          padding: '0 8px',
          backgroundColor: qb.surface,
          borderBottom: `1px solid ${qb.border}`,
        },
        '.qb-panel': {
          backgroundColor: qb.surface,
          border: `1px solid ${qb.border}`,
          borderRadius: theme('borderRadius.xs'),
          boxShadow: theme('boxShadow.panel'),
        },
      });

      addUtilities({
        // Monetary figures: fixed-width digits, right-aligned, no wrapping.
        '.num': {
          fontVariantNumeric: 'tabular-nums lining-nums',
          textAlign: 'right',
          whiteSpace: 'nowrap',
        },
        '.num-negative': {
          color: qb.negative,
        },
        '.cell-tight': { padding: '0 4px' },
        '.cell-flush': { padding: '0' },
        '.scrollbar-thin': {
          scrollbarWidth: 'thin',
          scrollbarColor: `${qb.borderStrong} transparent`,
        },
      });
    }),
  ],
};
