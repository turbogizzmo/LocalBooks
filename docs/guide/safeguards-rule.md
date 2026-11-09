# The FTC Safeguards Rule and LocalBooks

Tax preparers, and many bookkeepers and accountants, must keep a written information security
program under the US Federal Trade Commission's **Safeguards Rule**. This guide goes through the
rule's duties one by one and says, for each, what LocalBooks does to help and what stays your job.

> **Note:** This is a map, not legal advice. LocalBooks is one tool inside your security program;
> using it doesn't make a business compliant. Read the rule itself and talk to a professional if
> you're unsure what applies to you.

## Does the rule apply to you?

The rule (16 CFR Part 314) covers "financial institutions" under the FTC's jurisdiction, a term much
wider than banks. It names accountants and tax preparation services that complete income tax returns
as examples. Other firms, bookkeepers included, are covered when their work is significantly
engaged in financial activities; what you do counts, not what you call yourself. It protects
**customer information**: any record, in any form, with nonpublic personal financial information
about an individual who gets a financial service from you for personal, family or household use.

A firm that keeps customer information on **fewer than 5,000 consumers** doesn't need a written
risk assessment, the yearly penetration test and twice-yearly vulnerability scans, a written incident
response plan, or the yearly written report. Every other duty, including telling the FTC about a
breach, still applies.

Sources:

- The rule: [16 CFR Part 314, Standards for Safeguarding Customer Information](https://www.ecfr.gov/current/title-16/chapter-I/subchapter-C/part-314)
  (duties in § 314.4, definitions in § 314.2, the small-firm exception in § 314.6).
- The FTC's plain-language guide: [FTC Safeguards Rule: What Your Business Needs to Know](https://www.ftc.gov/business-guidance/resources/ftc-safeguards-rule-what-your-business-needs-know).

## Duty by duty

Section numbers are from § 314.4.

### (a) Put one person in charge

You must name a **Qualified Individual** to run the program; it can be you, an employee or an outside
firm. **LocalBooks:** nothing to set. In practice this person is usually an Admin in
**Settings › Users & Passwords** and keeps the recovery key.

### (b) Assess the risks in writing

You must assess the risks to customer information in writing (unless you're under 5,000 consumers)
and repeat it from time to time. **LocalBooks:** it can't do the assessment, but the list under
(c)(2) below says where LocalBooks keeps or creates copies of customer information, which is a
starting point.

### (c)(1) Control who gets in, and limit what they see

Only authorized people may get at customer information, and each only to what their job needs.
**LocalBooks:** turn on users ([Users, passwords and sensitive data](users-and-passwords.md)).
Everyone signs in with their own password, and the roles Admin, Standard and View only limit what
each can change. Vendor tax IDs and bank details are shown in full only to users allowed to see
sensitive data. Everyone is signed out after 15 minutes idle by default; an Admin can change that
limit or turn it off under **Sign out when idle**, so keep it on.
**Limits:** roles are enforced by LocalBooks, not by the file. Anyone who can read the company file
on disk can read most of it with another program, so control who can sign in to the computer and
reach the folder the file is in.

### (c)(2) Know where the data is

You must know what data, devices and systems you have and how much each matters. **LocalBooks**
keeps or makes copies of customer information in these places:

- **The company file** (`.db` or `.localbooks`), including every attachment: files you attach are
  stored inside it.
- **Automatic and manual backups**, in the `backups` folder beside the default company file
  ([Back up, restore and open company files](backups-and-company-files.md)).
- **Offsite backups**, in the folder you picked, encrypted.
- **Archives** made by Condense data, and its Before condense backup, in the same `backups` folder.
- **Exports** you save: reports and lists as CSV, Excel or PDF, forms as PDF, attachments saved to a
  folder or zip, ACH payment files, 1099 exports. These are plain files wherever you saved them.
- **Credentials** for bank downloads, email and remembered sign-ins: in the Mac Keychain or Windows
  Credential Manager (on Linux, a file only your account can read), never in the company file.

### (c)(3) Encrypt customer information, at rest and in transit

The rule asks for encryption of customer information you hold and send. Where that isn't feasible,
the Qualified Individual may approve other controls that work as well.
**LocalBooks, at rest:** with users on, vendor tax IDs and bank details are encrypted in the company
file (AES-256, key locked by each allowed user's password and the recovery key). Offsite backups are
encrypted as a whole before they leave LocalBooks.
**Limits:** the rest of the company file, its local backups and attachments are not encrypted by
LocalBooks yet (whole-file encryption is planned). Turn on full-disk encryption on every computer
that holds them: FileVault on a Mac, BitLocker or Device Encryption on Windows. Your Qualified
Individual can record that as the approved control. Exports are plain files: keep them on encrypted
disks too, and delete them when done.
**In transit:** bank downloads (SimpleFIN), updates and exchange-rate lookups use HTTPS. Email over
SMTP refuses to send a password without STARTTLS or TLS, but the message and its PDF are only as
private as your email provider makes them. Emailing a form through your own mail app is that app's
job. A company file kept on a network drive travels as the network sends it; ask whoever runs the
share whether it encrypts.

### (c)(4) Check the software you use

For software someone else wrote, you must have a way to evaluate or test its security.
**LocalBooks:** the source code is public, and [SECURITY.md](https://github.com/turbogizzmo/LocalBooks/blob/main/SECURITY.md)
lists each security claim with the code and the automated test behind it. Updates install only if
they carry the project's signature. Report a problem privately through the link in SECURITY.md.

### (c)(5) Multi-factor authentication

The rule asks for multi-factor authentication for anyone getting into an information system, unless
the Qualified Individual approves in writing other controls that are as secure.
**Limits:** LocalBooks has **no multi-factor sign-in**: a LocalBooks user signs in with a password
only. What you can do: require a strong password or Touch ID / Windows Hello on the computer's own
account, keep full-disk encryption on, leave **Remember me on this computer** off on shared
computers, and have your Qualified Individual record whether that combination is acceptable.

### (c)(6) Dispose of data you no longer need

Customer information must be disposed of no later than two years after you last used it to serve
that customer, unless you need it for business, the law requires you to keep it, or it can't
reasonably be pulled out. You must also review your retention policy from time to time.
**LocalBooks:** posted transactions are never deleted: that's what keeps the books trustworthy, and
tax and business records usually have to be kept anyway. What you can remove:

- **Condense data** replaces closed years' transactions with monthly summaries. The detail moves to
  an archive (and a Before condense backup) in the `backups` folder, which you can later delete or
  store elsewhere.
- A vendor's **tax ID** and **bank details** can be cleared on the vendor record, and **attachments**
  can be deleted.
- Old **backups** are files you can delete; offsite backup keeps only the number of copies you
  choose.
- Deleted content is overwritten inside the company file, not left in free space.

**Limits:** there is no command to erase one customer's history: names and amounts stay in the
transactions they're part of. Delete exports and old backups yourself.

### (c)(7) Manage changes

You must have procedures for managing changes. **LocalBooks:** updates ask before installing, save a
copy of your books first, and list their changes in the release notes (the changelog). Your own
procedures for changes to computers and networks are outside LocalBooks.

### (c)(8) Log what users do and catch tampering

You must monitor and log what authorized users do and detect unauthorized access, use or tampering.
**LocalBooks:** every posted entry records the signed-in user and the computer's user. Turning users
on, adding or changing users, password resets and recovery-key use are recorded too. The trail is
hash-chained and its latest link is kept outside the file, so **Accounting › Audit Log › Verify
ledger integrity** detects changes made behind LocalBooks' back
([Verify and correct the ledger](ledger-integrity.md)).
**Limits:** LocalBooks logs changes, not reads: it doesn't record who looked at a customer or ran a
report. Failed sign-ins aren't recorded.

### (d) Test and monitor the safeguards

You must test or monitor your safeguards regularly. Without continuous monitoring, that means a
penetration test every year and vulnerability scans at least every six months (not required under
5,000 consumers). **LocalBooks** helps you check its own part: **Verify ledger integrity**, the
**File Dentist** check and offsite **Test restore**.
**Limits:** testing your computers and network is yours to arrange.

### (e) Train your people

Staff need security awareness training, and security staff need to stay current.
**Limits:** outside LocalBooks. The guides in Help can be part of the training on how LocalBooks
protects data.

### (f) Oversee service providers

You must choose service providers who can protect the data, require it by contract, and check on
them. **LocalBooks** is not a service: there's no LocalBooks server and no account, and nothing is
sent anywhere unless you turn a feature on. The providers that matter are the ones **you** connect:
SimpleFIN for bank downloads, your email provider, the cloud service that syncs your offsite backup
folder (it sees only encrypted copies) and your payroll service.

### (g) Keep the program up to date

You must adjust the program as testing, changes in your business and new risks show.
**Limits:** outside LocalBooks. Read the changelog when you update, since a new release can change
what LocalBooks covers.

### (h) Have a written incident response plan

You need a written plan for responding to and recovering from a security event (not required under
5,000 consumers). **LocalBooks** gives your plan these steps:

- **Restore** the books from a backup or an offsite copy.
- **Reset passwords** and issue a **new recovery key** (the old one stops working) in
  **Settings › Users & Passwords**; turn off a user who has left.
- **Forget a remembered sign-in** on a computer that may be compromised.
- **Issue a new AI access token** if local AI access was turned on.
- **Verify ledger integrity** to see whether anything was changed.
- **Save diagnostics** for whoever is helping you: it holds logs and versions, no business data.

### (i) Report to your board or owner

At least once a year the Qualified Individual reports in writing to the board, or to a senior
officer if there's no board (not required under 5,000 consumers). **Limits:** outside LocalBooks.

### (j) Tell the FTC about a breach

If unencrypted customer information about **500 or more** consumers is taken without authorization,
you must notify the FTC through the form on its website as soon as possible and within 30 days of
finding out. This applies whatever your size. Information counts as unencrypted if the key was taken
too. **LocalBooks:** encrypted tax IDs and bank details are protected only while the passwords and
recovery key stay safe. **Limits:** the rest of the company file is unencrypted unless the disk is,
so a stolen laptop without full-disk encryption may be a notification event. State laws can add
their own notice duties.
