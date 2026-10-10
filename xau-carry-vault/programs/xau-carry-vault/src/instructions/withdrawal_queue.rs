use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Burn, CloseAccount, Mint, Token, TokenAccount, TransferChecked},
};

use crate::{
    constants::*,
    error::VaultError,
    state::{VaultV1State, WithdrawalRequestV1},
};

#[derive(Accounts)]
#[instruction(nonce: u64)]
pub struct RequestWithdrawalV1<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        seeds = [VAULT_SEED, vault_v1_state.usdc_mint.as_ref()],
        bump = vault_v1_state.bump,
        has_one = vxau_mint
    )]
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,

    #[account(
        seeds = [VXAU_MINT_SEED, vault_v1_state.key().as_ref()],
        bump,
        mint::authority = vault_v1_state,
        mint::decimals = RECEIPT_DECIMALS
    )]
    pub vxau_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = vxau_mint,
        associated_token::authority = user
    )]
    pub user_vxau_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        init,
        payer = user,
        space = 8 + WithdrawalRequestV1::INIT_SPACE,
        seeds = [
            WITHDRAWAL_REQUEST_SEED,
            vault_v1_state.key().as_ref(),
            user.key().as_ref(),
            &nonce.to_le_bytes()
        ],
        bump
    )]
    pub withdrawal_request: Box<Account<'info, WithdrawalRequestV1>>,

    #[account(
        init,
        payer = user,
        seeds = [
            WITHDRAWAL_ESCROW_SEED,
            withdrawal_request.key().as_ref()
        ],
        bump,
        token::mint = vxau_mint,
        token::authority = withdrawal_request
    )]
    pub withdrawal_escrow: Box<Account<'info, TokenAccount>>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handle_request(
    ctx: Context<RequestWithdrawalV1>,
    nonce: u64,
    shares: u64,
    min_assets_out: u64,
) -> Result<()> {
    require!(
        !ctx.accounts.vault_v1_state.withdrawals_paused,
        VaultError::WithdrawalsPaused
    );
    require!(shares > 0, VaultError::ZeroAmount);
    require!(min_assets_out > 0, VaultError::InvalidMinimumOutput);
    require!(
        ctx.accounts.user_vxau_ata.amount >= shares,
        VaultError::InsufficientReceipts
    );

    token::transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.user_vxau_ata.to_account_info(),
                mint: ctx.accounts.vxau_mint.to_account_info(),
                to: ctx.accounts.withdrawal_escrow.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        shares,
        RECEIPT_DECIMALS,
    )?;

    ctx.accounts
        .withdrawal_request
        .set_inner(WithdrawalRequestV1 {
            vault: ctx.accounts.vault_v1_state.key(),
            owner: ctx.accounts.user.key(),
            nonce,
            shares,
            min_assets_out,
            requested_at_slot: Clock::get()?.slot,
            bump: ctx.bumps.withdrawal_request,
        });

    // Receipt supply is unchanged until settlement.
    Ok(())
}

#[derive(Accounts)]
pub struct CompleteWithdrawalV1<'info> {
    // Anyone may pay to settle. Cancellation requires payer == owner.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: Bound to the stored request owner.
    /// Receives token outputs and closed-account rent.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,

    #[account(
        seeds = [VAULT_SEED, usdc_mint.key().as_ref()],
        bump = vault_v1_state.bump,
        has_one = usdc_mint,
        has_one = vxau_mint
    )]
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,

    #[account(
        constraint = usdc_mint.decimals == RECEIPT_DECIMALS
            @ VaultError::InvalidDecimals
    )]
    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [VXAU_MINT_SEED, vault_v1_state.key().as_ref()],
        bump,
        mint::authority = vault_v1_state,
        mint::decimals = RECEIPT_DECIMALS
    )]
    pub vxau_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [TOKEN_VAULT_SEED, vault_v1_state.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = vault_v1_state
    )]
    pub vault_v1_usdc_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        close = owner,
        seeds = [
            WITHDRAWAL_REQUEST_SEED,
            vault_v1_state.key().as_ref(),
            owner.key().as_ref(),
            &withdrawal_request.nonce.to_le_bytes()
        ],
        bump = withdrawal_request.bump,
        has_one = owner @ VaultError::InvalidWithdrawalRequest,
        constraint = withdrawal_request.vault == vault_v1_state.key()
            @ VaultError::InvalidWithdrawalRequest
    )]
    pub withdrawal_request: Box<Account<'info, WithdrawalRequestV1>>,

    #[account(
        mut,
        seeds = [
            WITHDRAWAL_ESCROW_SEED,
            withdrawal_request.key().as_ref()
        ],
        bump,
        token::mint = vxau_mint,
        token::authority = withdrawal_request
    )]
    pub withdrawal_escrow: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = usdc_mint,
        associated_token::authority = owner
    )]
    pub owner_usdc_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = payer,
        associated_token::mint = vxau_mint,
        associated_token::authority = owner
    )]
    pub owner_vxau_ata: Box<Account<'info, TokenAccount>>,

    /// CHECK: Canonical PDA. Settlement authenticates full strategy NAV;
    /// cancellation does not depend on strategy state.
    #[account(
        seeds = [STRATEGY_SEED, vault_v1_state.key().as_ref()],
        bump
    )]
    pub strategy_account: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_complete(ctx: Context<CompleteWithdrawalV1>, cancel: bool) -> Result<()> {
    let request = &ctx.accounts.withdrawal_request;
    let recorded_shares = request.shares;
    let escrow_balance = ctx.accounts.withdrawal_escrow.amount;

    require!(
        recorded_shares > 0 && escrow_balance >= recorded_shares,
        VaultError::InvalidWithdrawalEscrow
    );

    // Price before any burn. Never accept caller-supplied NAV.
    let assets_out = if cancel {
        require_keys_eq!(
            ctx.accounts.payer.key(),
            ctx.accounts.owner.key(),
            VaultError::UnauthorizedWithdrawalCancellation
        );
        0
    } else {
        require!(
            !ctx.accounts.vault_v1_state.withdrawals_paused,
            VaultError::WithdrawalsPaused
        );

        let nav = crate::strategy_nav::quote(
            ctx.accounts.vault_v1_state.key(),
            &ctx.accounts.vault_v1_state,
            &ctx.accounts.strategy_account.to_account_info(),
            ctx.accounts.vault_v1_usdc_account.amount,
            ctx.remaining_accounts,
        )?;
        let assets = if *ctx.accounts.strategy_account.owner == anchor_lang::system_program::ID {
            crate::share_math::assets_for_redemption(
                recorded_shares,
                ctx.accounts.vxau_mint.supply,
                nav.net_assets,
            )?
        } else {
            crate::strategy_accounting::redemption_quote(
                recorded_shares,
                ctx.accounts.vxau_mint.supply,
                nav,
                request.min_assets_out,
            )
            .map_err(crate::strategy_nav::accounting_error)?
        };

        require!(assets > 0, VaultError::RedemptionTooSmall);
        require!(
            assets >= request.min_assets_out,
            VaultError::MinimumOutputNotMet
        );
        require!(
            assets <= ctx.accounts.vault_v1_usdc_account.amount,
            VaultError::InsufficientLiquidity
        );

        assets
    };

    let vault_key = ctx.accounts.vault_v1_state.key();
    let owner_key = ctx.accounts.owner.key();
    let nonce = request.nonce.to_le_bytes();
    let bump = [request.bump];

    let request_seeds: &[&[u8]] = &[
        WITHDRAWAL_REQUEST_SEED,
        vault_key.as_ref(),
        owner_key.as_ref(),
        &nonce,
        &bump,
    ];

    // Return all shares on cancellation. On settlement, return
    // unsolicited surplus shares so escrow closure cannot be blocked
    // merely by someone transferring extra receipts into it.
    let refund_shares = if cancel {
        escrow_balance
    } else {
        escrow_balance
            .checked_sub(recorded_shares)
            .ok_or(VaultError::InvalidWithdrawalEscrow)?
    };

    if refund_shares > 0 {
        token::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.withdrawal_escrow.to_account_info(),
                    mint: ctx.accounts.vxau_mint.to_account_info(),
                    to: ctx.accounts.owner_vxau_ata.to_account_info(),
                    authority: ctx.accounts.withdrawal_request.to_account_info(),
                },
                &[request_seeds],
            ),
            refund_shares,
            RECEIPT_DECIMALS,
        )?;
    }

    if !cancel {
        token::burn(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                Burn {
                    mint: ctx.accounts.vxau_mint.to_account_info(),
                    from: ctx.accounts.withdrawal_escrow.to_account_info(),
                    authority: ctx.accounts.withdrawal_request.to_account_info(),
                },
                &[request_seeds],
            ),
            recorded_shares,
        )?;

        let mint_key = ctx.accounts.usdc_mint.key();
        let vault_bump = [ctx.accounts.vault_v1_state.bump];
        let vault_seeds: &[&[u8]] = &[VAULT_SEED, mint_key.as_ref(), &vault_bump];

        token::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.vault_v1_usdc_account.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.owner_usdc_ata.to_account_info(),
                    authority: ctx.accounts.vault_v1_state.to_account_info(),
                },
                &[vault_seeds],
            ),
            assets_out,
            RECEIPT_DECIMALS,
        )?;
    }

    token::close_account(CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        CloseAccount {
            account: ctx.accounts.withdrawal_escrow.to_account_info(),
            destination: ctx.accounts.owner.to_account_info(),
            authority: ctx.accounts.withdrawal_request.to_account_info(),
        },
        &[request_seeds],
    ))?;

    // Anchor closes the request after successful handler exit.
    // Every CPI rolls back together if any subsequent step fails.
    Ok(())
}
