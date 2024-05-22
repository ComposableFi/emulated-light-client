set -eu

url=https://api.mainnet-beta.solana.com
. api-url.sh

columns=$(tput cols)

while read signature; do
	out=raw-tx/$signature
	if [ -e $out ]; then
		echo $signature: already exists
		continue
	fi
	curl "$url" -X POST -H "Content-Type: application/json" -d '{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "getTransaction",
  "params": [
    "'$signature'",
    "json"
  ]
}' >tmp || exit
	if grep 'Too many requests' tmp; then
		exit 1
	fi
	head=$(head -c $((columns - ${#signature} - 5)) <tmp)
	echo "$signature: $head"
	mv tmp $out
	sleep 0.1
done <signatures
