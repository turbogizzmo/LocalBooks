import { forwardRef } from 'react';
import { api, useLedgerQuery } from '../lib/api.js';

/** The company's classes, kept current as they change. */
export function useClasses() {
  return useLedgerQuery(api.listClasses).data ?? [];
}

/** True once the company has an active class: only then do forms show a Class column. */
export const tracksClasses = (classes) => classes.some((c) => c.isActive);

export function classLabel(classes, id) {
  return classes.find((c) => c.id === id)?.fullName ?? '';
}

/**
 * Class dropdown: active classes indented under their parents. A line keeps a class chosen
 * earlier even if it has since been made inactive, so that one is listed too (marked inactive).
 */
const ClassSelect = forwardRef(function ClassSelect({ classes, value, onChange, className = 'qb-input w-full', ...rest }, ref) {
  const options = classes.filter((c) => c.isActive || c.id === value);
  return (
    <select
      ref={ref}
      value={value ?? ''}
      onChange={(e) => onChange(e.target.value === '' ? null : Number(e.target.value))}
      className={className}
      {...rest}
    >
      <option value="">—</option>
      {options.map((c) => (
        <option key={c.id} value={c.id}>
          {'  '.repeat(c.depth)}
          {c.name}
          {c.isActive ? '' : ' (inactive)'}
        </option>
      ))}
    </select>
  );
});

export default ClassSelect;
