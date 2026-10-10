use bytemuck::Zeroable;
use crate::gmsol_store::{accounts::{Market,Position,VirtualInventory},types::{Pool,Clocks}};
impl Default for Market { fn default()->Self{Zeroable::zeroed()} }
impl Default for Position { fn default()->Self{Zeroable::zeroed()} }
impl Default for VirtualInventory { fn default()->Self{Zeroable::zeroed()} }
impl Pool {pub fn is_pure(&self)->bool{self.is_pure!=0}}
impl Clocks {
 pub fn get(&self,kind:gmsol_model::ClockKind)->Option<i64>{
  use gmsol_model::ClockKind;
  Some(match kind{ClockKind::PriceImpactDistribution=>self.price_impact_distribution,
   ClockKind::Borrowing=>self.borrowing,ClockKind::Funding=>self.funding,
   ClockKind::AdlForLong=>self.adl_for_long,ClockKind::AdlForShort=>self.adl_for_short,_=>return None})
 }
}
