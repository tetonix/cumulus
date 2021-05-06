// Copyright 2019-2020 PureStake Inc.
// This file is part of Moonbeam.

// Moonbeam is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Moonbeam is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Moonbeam.  If not, see <http://www.gnu.org/licenses/>.

//! Pallet that allows block authors to include their identity in a block via an inherent.
//! Currently the author does not _prove_ their identity, just states it. So it should not be used,
//! for things like equivocation slashing that require authenticated authorship information.

#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
	decl_error, decl_module, decl_storage, ensure,
	traits::FindAuthor,
	weights::{DispatchClass, Weight},
	Parameter,
};
use frame_system::ensure_none;
use parity_scale_codec::{Decode, Encode};
#[cfg(feature = "std")]
use sp_inherents::ProvideInherentData;
use sp_inherents::{InherentData, InherentIdentifier, IsFatalError, ProvideInherent};
use sp_runtime::{
	ConsensusEngineId, DigestItem, RuntimeString, RuntimeAppPublic,
	traits::Member,
};
use log::debug;
// use sp_application_crypto::AppKey;

/// The given account ID is the author of the current block.
pub trait EventHandler<Author> {
	fn note_author(author: Author);
}

impl<T> EventHandler<T> for () {
	fn note_author(_author: T) {}
}

/// Permissions for what block author can be set in this pallet
pub trait CanAuthor<AuthorId> {
	fn can_author(author: &AuthorId) -> bool;
}
/// Default implementation where anyone can author, see and `author-*-filter` pallets for
/// additional implementations.
/// TODO Promote this is "implementing relay chain consensus in the nimbus framework."
impl<T> CanAuthor<T> for () {
	fn can_author(_: &T) -> bool {
		true
	}
}

pub trait Config: frame_system::Config {
	// This is copied from Aura. I wonder if I really need all those trait bounds. For now I'll leave them.
	/// The identifier type for an authority.
	type AuthorId: Member + Parameter;

	//TODO do we have any use for this converter?
	// It has to happen eventually to pay rewards to accountids and let account ids stake.
	// But is there any reason it needs to be included here? For now I won't use it as I'm
	// not staking or rewarding in this poc.
	// A type to convert between AuthorId and AccountId

	/// Other pallets that want to be informed about block authorship
	type EventHandler: EventHandler<Self::AuthorId>;

	/// A preliminary means of checking the validity of this author. This check is run before
	/// block execution begins when data from previous inherent is unavailable. This is meant to
	/// quickly invalidate blocks from obviously-invalid authors, although it need not rule out all
	/// invlaid authors. The final check will be made when executing the inherent.
	type PreliminaryCanAuthor: CanAuthor<Self::AuthorId>;

	/// The final word on whether the reported author can author at this height.
	/// This will be used when executing the inherent. This check is often stricter than the
	/// Preliminary check, because it can use more data.
	/// If the pallet that implements this trait depends on an inherent, that inherent **must**
	/// be included before this one.
	type FullCanAuthor: CanAuthor<Self::AuthorId>;
}

// If the AccountId type supports it, then this pallet can be BoundToRuntimeAppPublic
impl<T> sp_runtime::BoundToRuntimeAppPublic for Module<T>
where
	T: Config,
	T::AuthorId: RuntimeAppPublic,
{
	type Public = T::AuthorId;
}

decl_error! {
	pub enum Error for Module<T: Config> {
		/// Author already set in block.
		AuthorAlreadySet,
		/// The author in the inherent is not an eligible author.
		CannotBeAuthor,
	}
}

decl_storage! {
	trait Store for Module<T: Config> as Author {
		/// Author of current block.
		Author: Option<T::AuthorId>;
	}
}

decl_module! {
	pub struct Module<T: Config> for enum Call where origin: T::Origin {
		type Error = Error<T>;

		fn on_initialize() -> Weight {
			<Author<T>>::kill();
			0
		}

		/// Inherent to set the author of a block
		#[weight = (
			0,
			DispatchClass::Mandatory
		)]
		fn set_author(origin, author: T::AuthorId) {

			ensure_none(origin)?;
			debug!(target: "author-inherent", "Executing Author inherent");
			ensure!(<Author<T>>::get().is_none(), Error::<T>::AuthorAlreadySet);
			debug!(target: "author-inherent", "Author was not already set");
			ensure!(T::FullCanAuthor::can_author(&author), Error::<T>::CannotBeAuthor);
			debug!(target: "author-inherent", "I can be author!");

			// Update storage
			Author::<T>::put(&author);

			// Add a digest item so Apps can detect the block author
			// For now we use the Consensus digest item.
			// Maybe this will change later.
			frame_system::Pallet::<T>::deposit_log(DigestItem::<T::Hash>::Consensus(
				ENGINE_ID,
				author.encode(),
			));

			// Notify any other pallets that are listening (eg rewards) about the author
			T::EventHandler::note_author(author);
		}
	}
}

impl<T: Config> FindAuthor<T::AuthorId> for Module<T> {
	fn find_author<'a, I>(_digests: I) -> Option<T::AuthorId>
	where
		I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
	{
		// We don't use the digests at all.
		// This will only return the correct author _after_ the authorship inherent is processed.
		<Author<T>>::get()
	}
}

// Can I express this as `*b"auth"` like we do for the inherent id?
pub const ENGINE_ID: ConsensusEngineId = [b'a', b'u', b't', b'h'];

pub const INHERENT_IDENTIFIER: InherentIdentifier = *b"author__";

#[derive(Encode)]
#[cfg_attr(feature = "std", derive(Debug, Decode))]
pub enum InherentError {
	Other(RuntimeString),
}

impl IsFatalError for InherentError {
	fn is_fatal_error(&self) -> bool {
		match *self {
			InherentError::Other(_) => true,
		}
	}
}

impl InherentError {
	/// Try to create an instance ouf of the given identifier and data.
	#[cfg(feature = "std")]
	pub fn try_from(id: &InherentIdentifier, data: &[u8]) -> Option<Self> {
		if id == &INHERENT_IDENTIFIER {
			<InherentError as parity_scale_codec::Decode>::decode(&mut &data[..]).ok()
		} else {
			None
		}
	}
}

/// The type of data that the inherent will contain.
pub type InherentType<T> = <T as Config>::AuthorId;

/// A thing that an outer node could use to inject the inherent data.
/// This should be used in simple uses of the author inherent (eg permissionless authoring)
/// When using the full nimbus system, we are manually inserting the  inherent.
#[cfg(feature = "std")]
pub struct InherentDataProvider<AuthorId>(pub AuthorId);

#[cfg(feature = "std")]
impl<AuthorId: Encode> ProvideInherentData for InherentDataProvider<AuthorId> {
	fn inherent_identifier(&self) -> &'static InherentIdentifier {
		&INHERENT_IDENTIFIER
	}

	fn provide_inherent_data(
		&self,
		inherent_data: &mut InherentData,
	) -> Result<(), sp_inherents::Error> {
		inherent_data.put_data(INHERENT_IDENTIFIER, &self.0)
	}

	fn error_to_string(&self, error: &[u8]) -> Option<String> {
		InherentError::try_from(&INHERENT_IDENTIFIER, error).map(|e| format!("{:?}", e))
	}
}

impl<T: Config> ProvideInherent for Module<T> {
	type Call = Call<T>;
	type Error = InherentError;
	const INHERENT_IDENTIFIER: InherentIdentifier = INHERENT_IDENTIFIER;

	fn is_inherent_required(_: &InherentData) -> Result<Option<Self::Error>, Self::Error> {
		// Return Ok(Some(_)) unconditionally because this inherent is required in every block
		// If it is not found, throw an AuthorInherentRequired error.
		Ok(Some(InherentError::Other(
			sp_runtime::RuntimeString::Borrowed("AuthorInherentRequired"),
		)))
	}

	fn create_inherent(data: &InherentData) -> Option<Self::Call> {
		// Grab the Vec<u8> labelled with "author__" from the map of all inherent data
		let author_raw = data
			.get_data::<InherentType<T>>(&INHERENT_IDENTIFIER);

		debug!("In create_inherent (runtime side). data is");
		debug!("{:?}", author_raw);

		let author = author_raw
			.expect("Gets and decodes authorship inherent data")?;

		//TODO we need to make the author _prove_ their identity, not just claim it.
		// we should have them sign something here. Best idea so far: parent block hash.

		// Decode the Vec<u8> into an account Id
		// let author =
		// 	T::AuthorId::decode(&mut &author_raw[..]).expect("Decodes author raw inherent data");

		Some(Call::set_author(author))
	}

	fn check_inherent(call: &Self::Call, _data: &InherentData) -> Result<(), Self::Error> {
		// We only check this pallet's inherent.
		if let Self::Call::set_author(claimed_author) = call {
			ensure!(
				T::PreliminaryCanAuthor::can_author(&claimed_author),
				InherentError::Other(sp_runtime::RuntimeString::Borrowed("Cannot Be Author"))
			);
		}

		Ok(())
	}

	fn is_inherent(call: &Self::Call) -> bool {
		matches!(call, Call::set_author(_))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate as author_inherent;

	use frame_support::{
		assert_noop, assert_ok, parameter_types,
		traits::{OnFinalize, OnInitialize},
	};
	use sp_core::H256;
	use sp_io::TestExternalities;
	use sp_runtime::{
		testing::Header,
		traits::{BlakeTwo256, IdentityLookup},
	};

	pub fn new_test_ext() -> TestExternalities {
		let t = frame_system::GenesisConfig::default()
			.build_storage::<Test>()
			.unwrap();
		TestExternalities::new(t)
	}

	type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Test>;
	type Block = frame_system::mocking::MockBlock<Test>;

	// Configure a mock runtime to test the pallet.
	frame_support::construct_runtime!(
		pub enum Test where
			Block = Block,
			NodeBlock = Block,
			UncheckedExtrinsic = UncheckedExtrinsic,
		{
			System: frame_system::{Pallet, Call, Config, Storage, Event<T>},
			AuthorInherent: author_inherent::{Pallet, Call, Storage, Inherent},
		}
	);

	parameter_types! {
		pub const BlockHashCount: u64 = 250;
	}
	impl frame_system::Config for Test {
		type BaseCallFilter = ();
		type BlockWeights = ();
		type BlockLength = ();
		type DbWeight = ();
		type Origin = Origin;
		type Index = u64;
		type BlockNumber = u64;
		type Call = Call;
		type Hash = H256;
		type Hashing = BlakeTwo256;
		type AccountId = u64;
		type Lookup = IdentityLookup<Self::AccountId>;
		type Header = Header;
		type Event = ();
		type BlockHashCount = BlockHashCount;
		type Version = ();
		type PalletInfo = PalletInfo;
		type AccountData = ();
		type OnNewAccount = ();
		type OnKilledAccount = ();
		type SystemWeightInfo = ();
		type SS58Prefix = ();
		type OnSetCode = ();
	}
	impl Config for Test {
		type AuthorId = u64;
		type EventHandler = ();
		type PreliminaryCanAuthor = ();
		type FullCanAuthor = ();
	}

	pub fn roll_to(n: u64) {
		while System::block_number() < n {
			System::on_finalize(System::block_number());
			System::set_block_number(System::block_number() + 1);
			System::on_initialize(System::block_number());
			AuthorInherent::on_initialize(System::block_number());
		}
	}

	#[test]
	fn set_author_works() {
		new_test_ext().execute_with(|| {
			assert_ok!(AuthorInherent::set_author(Origin::none(), 1));
			roll_to(1);
			assert_ok!(AuthorInherent::set_author(Origin::none(), 1));
			roll_to(2);
		});
	}

	#[test]
	fn must_be_inherent() {
		new_test_ext().execute_with(|| {
			assert_noop!(
				AuthorInherent::set_author(Origin::signed(1), 1),
				sp_runtime::DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn double_author_fails() {
		new_test_ext().execute_with(|| {
			assert_ok!(AuthorInherent::set_author(Origin::none(), 1));
			assert_noop!(
				AuthorInherent::set_author(Origin::none(), 1),
				Error::<Test>::AuthorAlreadySet
			);
		});
	}
}