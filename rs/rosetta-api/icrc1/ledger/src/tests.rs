use crate::{InitArgs, Ledger};
use ic_base_types::PrincipalId;
use ic_icrc1::Operation;
use ic_icrc1::Transaction;
use ic_ledger_canister_core::archive::ArchiveOptions;
use ic_ledger_canister_core::ledger::{LedgerContext, LedgerTransaction, TxApplyError};
use ic_ledger_core::approvals::{Allowance, Approvals};
use ic_ledger_core::timestamp::TimeStamp;
use ic_ledger_core::Tokens;
use icrc_ledger_types::icrc::generic_metadata_value::MetadataValue as Value;
use icrc_ledger_types::icrc1::account::Account;

use ic_icrc1_ledger_sm_tests::{
    ARCHIVE_TRIGGER_THRESHOLD, BLOB_META_KEY, BLOB_META_VALUE, FEE, INT_META_KEY, INT_META_VALUE,
    MINTER, NAT_META_KEY, NAT_META_VALUE, NUM_BLOCKS_TO_ARCHIVE, TEXT_META_KEY, TEXT_META_VALUE,
    TOKEN_NAME, TOKEN_SYMBOL,
};

use std::time::Duration;

fn test_account_id(n: u64) -> Account {
    Account {
        owner: PrincipalId::new_user_test_id(n).into(),
        subaccount: None,
    }
}

fn tokens(n: u64) -> Tokens {
    Tokens::from_e8s(n)
}

fn ts(n: u64) -> TimeStamp {
    TimeStamp::from_nanos_since_unix_epoch(n)
}

fn default_init_args() -> InitArgs {
    InitArgs {
        minting_account: MINTER,
        fee_collector_account: None,
        initial_balances: [].to_vec(),
        transfer_fee: FEE,
        token_name: TOKEN_NAME.to_string(),
        token_symbol: TOKEN_SYMBOL.to_string(),
        metadata: vec![
            Value::entry(NAT_META_KEY, NAT_META_VALUE),
            Value::entry(INT_META_KEY, INT_META_VALUE),
            Value::entry(TEXT_META_KEY, TEXT_META_VALUE),
            Value::entry(BLOB_META_KEY, BLOB_META_VALUE),
        ],
        archive_options: ArchiveOptions {
            trigger_threshold: ARCHIVE_TRIGGER_THRESHOLD as usize,
            num_blocks_to_archive: NUM_BLOCKS_TO_ARCHIVE as usize,
            node_max_memory_size_bytes: None,
            max_message_size_bytes: None,
            controller_id: PrincipalId::new_user_test_id(100),
            cycles_for_archive_creation: None,
            max_transactions_per_response: None,
        },
        max_memo_length: None,
    }
}

#[test]
fn test_approvals_are_not_cumulative() {
    let now = ts(12345678);

    let mut ctx = Ledger::from_init_args(default_init_args(), now);

    let from = test_account_id(1);
    let spender = test_account_id(2);

    ctx.balances_mut().mint(&from, tokens(100_000)).unwrap();

    let approved_amount = 150_000;
    let fee = 10_000;

    let tr = Transaction {
        operation: Operation::Approve {
            from,
            spender,
            amount: approved_amount,
            expected_allowance: None,
            expires_at: None,
            fee: Some(fee),
        },
        created_at_time: None,
        memo: None,
    };
    tr.apply(&mut ctx, now, Tokens::ZERO).unwrap();

    assert_eq!(ctx.balances().account_balance(&from), tokens(90_000));
    assert_eq!(ctx.balances().account_balance(&spender), tokens(0));

    assert_eq!(
        ctx.approvals().allowance(&from, &spender, now),
        Allowance {
            amount: tokens(approved_amount),
            expires_at: None
        },
    );

    let new_allowance = 200_000;

    let expiration = now + Duration::from_secs(300);
    let tr = Transaction {
        operation: Operation::Approve {
            from,
            spender,
            amount: new_allowance,
            expected_allowance: None,
            expires_at: Some(expiration),
            fee: Some(fee),
        },
        created_at_time: None,
        memo: None,
    };
    tr.apply(&mut ctx, now, Tokens::ZERO).unwrap();

    assert_eq!(ctx.balances().account_balance(&from), tokens(80_000));
    assert_eq!(ctx.balances().account_balance(&spender), tokens(0));
    assert_eq!(
        ctx.approvals().allowance(&from, &spender, now),
        Allowance {
            amount: tokens(new_allowance),
            expires_at: Some(expiration)
        }
    );
}

#[test]
fn test_approval_expiration_override() {
    let now = ts(1000);

    let mut ctx = Ledger::from_init_args(default_init_args(), now);

    let from = test_account_id(1);
    let spender = test_account_id(2);

    ctx.balances_mut().mint(&from, tokens(200_000)).unwrap();

    let approve = |amount: u64, expires_at: Option<TimeStamp>| Operation::Approve {
        from,
        spender,
        amount,
        expected_allowance: None,
        expires_at,
        fee: Some(10_000),
    };
    let tr = Transaction {
        operation: approve(100_000, Some(ts(2000))),
        created_at_time: None,
        memo: None,
    };
    tr.apply(&mut ctx, now, Tokens::ZERO).unwrap();

    assert_eq!(
        ctx.approvals().allowance(&from, &spender, now),
        Allowance {
            amount: tokens(100_000),
            expires_at: Some(ts(2000))
        },
    );

    let tr = Transaction {
        operation: approve(200_000, Some(ts(1500))),
        created_at_time: None,
        memo: None,
    };
    tr.apply(&mut ctx, now, Tokens::ZERO).unwrap();

    assert_eq!(
        ctx.approvals().allowance(&from, &spender, now),
        Allowance {
            amount: tokens(200_000),
            expires_at: Some(ts(1500))
        },
    );

    let tr = Transaction {
        operation: approve(300_000, Some(ts(2500))),
        created_at_time: None,
        memo: None,
    };
    tr.apply(&mut ctx, now, Tokens::ZERO).unwrap();

    assert_eq!(
        ctx.approvals().allowance(&from, &spender, now),
        Allowance {
            amount: tokens(300_000),
            expires_at: Some(ts(2500))
        },
    );

    // The expiration is in the past, the allowance is rejected.
    let tr = Transaction {
        operation: approve(100_000, Some(ts(500))),
        created_at_time: None,
        memo: None,
    };
    assert_eq!(
        tr.apply(&mut ctx, now, Tokens::ZERO).unwrap_err(),
        TxApplyError::ExpiredApproval { now }
    );

    assert_eq!(
        ctx.approvals().allowance(&from, &spender, now),
        Allowance {
            amount: tokens(300_000),
            expires_at: Some(ts(2500))
        },
    );
}

#[test]
fn test_approval_no_fee_on_reject() {
    let now = ts(1000);

    let mut ctx = Ledger::from_init_args(default_init_args(), now);

    let from = test_account_id(1);
    let spender = test_account_id(2);

    ctx.balances_mut().mint(&from, tokens(20_000)).unwrap();

    let tr = Transaction {
        operation: Operation::Approve {
            from,
            spender,
            amount: 1_000,
            expected_allowance: None,
            expires_at: Some(ts(1)),
            fee: Some(10_000),
        },
        created_at_time: Some(1000),
        memo: None,
    };

    assert_eq!(
        tr.apply(&mut ctx, now, Tokens::ZERO).unwrap_err(),
        TxApplyError::ExpiredApproval { now }
    );

    assert_eq!(
        ctx.approvals().allowance(&from, &spender, now),
        Allowance::default(),
    );

    assert_eq!(ctx.balances().account_balance(&from), tokens(20_000));
}