// Help, part 3: topics on Employees & time (the number in the file name sets the order) and the help for its screens.
// Each topic’s own `area` decides its heading under All topics.

/** What each screen is for (`help`) and its main guide (`guide`, docs/guide/<id>.md), shown at the top of Help. */
export const screens = {
  'employees/time': {
    help: 'Hours worked by employees, entered one activity at a time or as a weekly timesheet. Time does not post to your books; billable hours can be invoiced.',
    guide: 'time-tracking',
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
];
