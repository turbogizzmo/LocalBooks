# NACHA test vectors

These files come unchanged from moov-io/ach (https://github.com/moov-io/ach, `test/testdata`),
Copyright The Moov Authors, licensed under the Apache License 2.0. `nacha/tests.rs` reads them to
check that our validator agrees with moov's about valid and invalid files.

- `ppd-debit.ach`: a valid PPD debit file, with trailing spaces trimmed (the test pads them back).
- `ppd-mixedDebitCredit.ach`: a valid PPD file mixing a debit and two credits (service class 200).
- `ppd-debit-invalid-entryDetail-checkDigit.ach`: the first file with a bad routing check digit.
- `short-line.ach`: a file control record shorter than 94 characters.
