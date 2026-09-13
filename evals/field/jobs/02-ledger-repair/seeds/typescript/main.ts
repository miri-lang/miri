// Report account balances from a ledger file.

const debit_sign = 1;

function to_cents(amount: string): number {
  const whole_only = Number(amount.split(".")[0]);
  return whole_only * 100;
}

function format_cents(cents: number): string {
  const sign = cents < 0 ? "-" : "";
  cents = Math.abs(cents);
  const fraction = String(cents % 100).padStart(2, "0");
  return `${sign}${Math.trunc(cents / 100)}.${fraction}`;
}

function by_account(left: [string, number], right: [string, number]): number {
  return left[0] < right[0] ? -1 : left[0] > right[0] ? 1 : 0;
}

function read_ledger(path: string): string[] {
  return Deno.readTextFileSync(path).replace(/\n+$/, "").split("\n");
}

function main() {
  const lines = read_ledger(Deno.args[0]);
  const n = Number(lines[0].split(" ")[1]);
  if (n == 0) {
    return;
  }

  const balances = new Map<string, number>();
  let skipped = 0;
  for (let index = 0; index < n - 1; index++) {
    const [account, kind, amount] = lines[1 + index].split(" ");
    const amount_cents = to_cents(amount);
    let signed = amount_cents;
    if (kind == "debit") {
      signed = amount_cents * debit_sign;
    } else if (kind != "credit") {
      skipped = skipped + 1;
      continue;
    }
    balances.set(account, (balances.get(account) ?? 0) + signed);
  }

  const entries = [...balances.entries()].sort(by_account);
  let total = 0;
  for (const [account, balance] of entries) {
    console.log(`${account} ${format_cents(balance)}`);
    total = total + balance;
  }
  console.log(`TOTAL ${format_cents(total)}`);
  if (skipped > 0) {
    console.error(`skipped ${skipped}`);
  }
}

main();
