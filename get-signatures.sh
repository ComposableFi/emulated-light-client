set -eu

url=https://api.mainnet-beta.solana.com
. api-url.sh

columns=$(tput cols)

# C6r1VEbn3mSpecgrZ7NdBvWUtYVJWrDPv4uU9Xs956gc sigverify
# FufGpHqMQgGVjtMH9AV8YMrJYq8zaK6USRsJkZP4yDjo write-account
# 2HLLVco5HvwWriNbUhmVwA2pCetRkpgrqwnjcsZdyTKT solana-ibc
address=C6r1VEbn3mSpecgrZ7NdBvWUtYVJWrDPv4uU9Xs956gc
file=sigverify

curl "$url" -X POST -H "Content-Type: application/json" -d '
  {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [
      "'$address'",
      {
        "limit": 1000000
      }
    ]
  }
' >>$file-sigs
