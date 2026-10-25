# Changelog

What changed in each LocalBooks release. A release's section here becomes its release notes on
GitHub and in the app's update window (see `.github/workflows/release.yml`), so write it for the
people using LocalBooks.

## [0.5.1]

- **Tax IDs from newer QuickBooks files.** QuickBooks keeps vendor tax IDs encrypted with its own
  key, which LocalBooks can't read. Books imported before 0.5.0 could show that unreadable text as a
  vendor's tax ID (and on the 1099 report). It is now removed automatically when you open the file,
  or at your next sign-in if users are on. Re-enter those IDs from the vendors' W-9 forms. Real tax
  IDs and notes are never touched.

## [0.5.0]

- **Users and passwords (optional).** Turn them on in Settings › Users & Passwords: everyone signs
  in with their own password and a role (Admin, Standard or View only). Save the recovery key you
  are shown; it is the only way back in if every Admin forgets their password. LocalBooks signs
  everyone out after 15 minutes without activity (an Admin can change this), and can remember your
  sign-in on your own Mac. Files without users work exactly as before.
- **Encrypted tax IDs.** With users on, vendor tax IDs (SSNs and EINs) are encrypted and shown as
  `***-**-6789` to anyone not allowed to see sensitive data. The 1099 report masks them on screen;
  its CSV has them in full for people who may see them.
- **Accountability.** The audit trail now records who did what, including changes to users and
  passwords.
- **Fixed:** a foreign-currency payment could be refused after earlier partial payments on the same
  invoice were voided.
- Behind the scenes: much stronger automated testing of the ledger (thousands of random
  transaction sequences checked against the accounting rules) and saved copies of every report to
  catch unintended changes.
