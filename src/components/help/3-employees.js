// Help, part 3: topics on Employees & time (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  'employees/time': {
    help: 'Hours worked by employees, entered one activity at a time or as a weekly timesheet. Time does not post to your books; billable hours can be invoiced.',
    guide: 'time-tracking',
  },
  'employees/mileage': {
    help: 'Trips driven in each vehicle, the mileage rates you enter, and billing billable trips to customers. Trips do not post to your books; a billed trip posts through its invoice.',
    guide: 'mileage',
  },
};

/**
 * Topics: two sentences each, describing what LocalBooks actually does. `views` lists the
 * screens a topic is about (shown under "On this screen"); `go` opens the place to do it, with an
 * optional `action` (open a form, a Settings tab or section); `guide` is its step-by-step guide.
 */
export const topics = [
  {
    id: 'time', area: 'employees', views: ['employees/time'],
    title: 'Track time',
    body: 'Enter a single time activity, or a weekly timesheet with hours for each day; mark hours billable with a customer and hourly rate to invoice them later. Time does not post to your books by itself.',
    keywords: 'hours timesheet employee billable weekly activity',
    go: { label: 'Weekly timesheet', view: 'employees/time', action: 'weekly-timesheet' },
    guide: 'time-tracking',
  },
  {
    id: 'mileage', area: 'employees', views: ['employees/mileage', 'reports'],
    title: 'Track vehicle mileage',
    body: 'Record trips by vehicle with odometer readings or total miles, a customer or job and notes; the deductible amount is miles × the mileage rate you entered for the trip date (find the official rate at irs.gov). Bill billable trips on an invoice at your own sales rate, and see Mileage by Vehicle and Mileage by Job under Reports › Jobs.',
    keywords: 'mileage miles vehicle car truck trip odometer irs standard mileage rate deduction billable reimbursement',
    go: { label: 'Enter a trip', view: 'employees/mileage', action: 'mileage-trip' },
    guide: 'mileage',
  },
];
