const MONTH_NAMES = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** The first month of the fiscal year (1–12) from the `fiscal_year_start` preference ("07-01"). */
export function fiscalStartMonth(pref) {
  const m = Number(String(pref ?? '01-01').slice(0, 2));
  return Number.isInteger(m) && m >= 1 && m <= 12 ? m : 1;
}

/**
 * The fiscal years New budget offers: the one containing `todayIso`, the one before and the two
 * after, each as its first day (what the budget is created from) and its months, e.g.
 * "Jul 2026 – Jun 2027". A calendar fiscal year reads "Jan – Dec 2026".
 */
export function budgetYearChoices(todayIso, startMonth) {
  const [ty, tm] = todayIso.split('-').map(Number);
  const current = tm >= startMonth ? ty : ty - 1;
  return [current - 1, current, current + 1, current + 2].map((y) => {
    const start = `${y}-${String(startMonth).padStart(2, '0')}-01`;
    const endMonth = ((startMonth + 10) % 12) + 1;
    const endYear = startMonth === 1 ? y : y + 1;
    const label = startMonth === 1 ? `Jan – Dec ${y}` : `${MONTH_NAMES[startMonth - 1]} ${y} – ${MONTH_NAMES[endMonth - 1]} ${endYear}`;
    return { start, label, current: y === current };
  });
}
