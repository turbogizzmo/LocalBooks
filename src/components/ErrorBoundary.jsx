import { Component } from 'react';
import { api } from '../lib/api.js';
import { crashDetails, crashLogLine } from '../lib/crash.js';

/**
 * Keeps one screen's failure to itself. Without it, an error while React draws any component takes
 * the whole app down to a blank window, with every other open window and half-entered form.
 *
 * Around one window inside Tillhouse (`onClose` given), the other windows carry on; **Try again**
 * draws the window afresh and **Close window** closes it. Around the whole app, the only way on is
 * to reload. What was typed into the failed screen is gone either way, and the message says so.
 *
 * `where`: what failed, in words ("the Invoices window"), for the message and the diagnostics log.
 */
export default class ErrorBoundary extends Component {
  constructor(props) {
    super(props);
    this.state = { error: null, details: '', copied: false, attempt: 0 };
  }

  static getDerivedStateFromError(error) {
    return { error };
  }

  componentDidCatch(error, info) {
    const where = this.props.where ?? 'this window';
    this.setState({ details: crashDetails(where, error, info?.componentStack) });
    // The log holds no business data: the kind of error and the components, not its message.
    try {
      api.logScreenError(crashLogLine(where, error, info?.componentStack)).catch(() => {});
    } catch {
      // Outside the app shell there is no log to write to.
    }
  }

  retry = () => this.setState((s) => ({ error: null, details: '', copied: false, attempt: s.attempt + 1 }));

  copy = async () => {
    try {
      await navigator.clipboard.writeText(this.state.details);
      this.setState({ copied: true });
    } catch {
      this.setState({ copied: false });
    }
  };

  render() {
    const { error, details, copied, attempt } = this.state;
    const { where = 'this window', onClose, children } = this.props;
    // A new key each try, so the screen starts again from nothing rather than from what failed.
    if (!error) return <ErrorBoundaryContent key={attempt}>{children}</ErrorBoundaryContent>;
    const wholeApp = !onClose;
    return (
      <div className={`flex items-start justify-center bg-qb-canvas p-4 ${wholeApp ? 'h-full' : 'min-h-full'}`}>
        <section className="qb-panel w-full max-w-[560px]" role="alert">
          <h2 className="border-b border-qb-border px-3 py-2 text-lg font-semibold">{wholeApp ? 'Tillhouse ran into a problem' : 'This window ran into a problem'}</h2>
          <div className="flex flex-col gap-2 px-3 py-2 text-sm">
            <p className="text-md">
              Tillhouse could not show {where}. Your books are safe: everything already saved is still saved. Anything typed {wholeApp ? 'into a form' : 'into this window'} and
              not yet saved was lost.
            </p>
            {!wholeApp && <p className="text-qb-ink-muted">Your other windows are not affected.</p>}
            <div className="flex flex-wrap items-center gap-2">
              {wholeApp ? (
                <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={() => window.location.reload()}>
                  Reload Tillhouse
                </button>
              ) : (
                <>
                  <button type="button" className="qb-btn qb-btn-primary h-row-lg px-3" onClick={this.retry}>
                    Try again
                  </button>
                  <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={onClose}>
                    Close window
                  </button>
                </>
              )}
              <button type="button" className="qb-btn qb-btn-secondary h-row-lg px-3" onClick={this.copy}>
                {copied ? '✓ Copied' : 'Copy details'}
              </button>
            </div>
            <p className="text-qb-ink-muted">If it happens again, choose Help › Save Diagnostics… and send the file, with the details, to whoever helps you with Tillhouse.</p>
            <details className="text-xs">
              <summary className="cursor-pointer text-qb-ink-muted">Technical details</summary>
              <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-qb-negative">{details || String(error?.message ?? error)}</pre>
            </details>
          </div>
        </section>
      </div>
    );
  }
}

function ErrorBoundaryContent({ children }) {
  return children;
}
