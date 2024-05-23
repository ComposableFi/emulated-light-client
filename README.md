Tools for gathering statistics about Solana IBC bridge

* tools/common.py includes `OWN_PROGRAMS_BY_ADDRESS` map which lists
  addresses of the three programs which are part of the bridge.

* tools/fetch-signatures.py fetches last 100k signatures for those
  addresses.  The signatures are saved to data/signatures directory.
  Note there’s data/signatures.tar.xz file with already fetched
  signatures.

* tools/fetch-transactions.py fetches all of the transactions
  corresponding to those signatures.  The transactions are saved to
  data/raw-tx directory.  Note there’s data/raw-tx.tar.xz file with
  already fetched signatures.

  It also prints at the end range of Solana slots which is common for
  all three addresses.  This range should be used to update
  `START_SLOT` and `END_SLOT` in common.py.

  Last 100k transactions sent to sigverify may go further in the past
  than 100k transactions sent to solana-ibc.  To avoid being unable to
  correlate such calls, it may be beneficial to look at only subset of
  the transactions.

* tools/process-raw.py preprocesses the transaction to format which is
  a bit easier to read and work with.  For example, it resolves
  account indices.  It saves processed transactions to data/tx
  directory.

* tools/concat.py further processes the transactions and concatenates
  them into a single file in data/txs.json.  It filters out any
  transactions which resulted in an error and those which are outside
  of a slot range `common.START_SLOT..=common.END_SLOT`.

* tools/collect-stats.py processes the data and collect various
  statistics writing them out as CSV files into output directory.
