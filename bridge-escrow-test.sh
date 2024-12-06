rm -fr ./test-ledger

solana-test-validator > /dev/null 2>&1  &
VALIDATOR_PID=$!

echo "Solana test validator started in the background with PID $VALIDATOR_PID."

sleep 5

echo "Running Debug and Deployment"

# Default behavior
SKIP_DEPLOY=false

# Parse command-line arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --skip-deploy) SKIP_DEPLOY=true ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done


solana config set --url http://127.0.0.1:8899

if [ "$SKIP_DEPLOY" = false ]; then
    anchor build -p bridge_escrow
    solana program deploy target/deploy/bridge_escrow.so --program-id target/deploy/bridge_escrow-keypair.json
else
    echo "Skipping Deployment"
fi

cargo test -p bridge-escrow -- --nocapture

echo "Stopping solana-test-validator..."
kill $VALIDATOR_PID

wait $VALIDATOR_PID
echo "Solana test validator has been stopped."