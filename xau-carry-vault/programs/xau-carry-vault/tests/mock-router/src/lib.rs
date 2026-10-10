//! Test-only CPI router; never deploy. Token transfers still execute real SPL/Token2022 code.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{entrypoint,entrypoint::ProgramResult};
entrypoint!(process_instruction);
fn process_instruction(program:&Pubkey,a:&[AccountInfo],data:&[u8])->ProgramResult {
    if data.len()<19 || a.len()<17 {return Err(anchor_lang::solana_program::program_error::ProgramError::InvalidInstructionData);}
    let tail=&data[data.len()-19..];
    let input=u64::from_le_bytes(tail[..8].try_into().unwrap());
    let output=u64::from_le_bytes(tail[8..16].try_into().unwrap());
    let input_program=if *a[7].owner==anchor_spl::token::ID {a[0].key()}else{a[10].key()};
    let output_program=if *a[8].owner==anchor_spl::token::ID {a[0].key()}else{a[10].key()};
    anchor_spl::token_interface::transfer_checked(CpiContext::new(input_program,anchor_spl::token_interface::TransferChecked {
        from:a[3].clone(),mint:a[7].clone(),to:a[13].clone(),authority:a[2].clone()}),input,6)?;
    let (_,bump)=Pubkey::find_program_address(&[b"mock_pool"],program);let bump=[bump];let seeds:&[&[u8]]=&[b"mock_pool",&bump];
    anchor_spl::token_interface::transfer_checked(CpiContext::new_with_signer(output_program,anchor_spl::token_interface::TransferChecked {
        from:a[14].clone(),mint:a[8].clone(),to:a[6].clone(),authority:a[15].clone()},&[seeds]),output,6)?;
    Ok(())
}
