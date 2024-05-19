#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[frame_support::pallet(dev_mode)]
pub mod pallet {
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
	use frame_support::sp_runtime::SaturatedConversion;

	use frame_support::traits::{Currency};

	type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[derive(Clone, Encode, Decode, PartialEq, Copy, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	#[scale_info(skip_type_params(T))]
	pub struct Collectible<T: Config> {
		pub unique_id: u64,
		pub price: Option<BalanceOf<T>>,
		pub color: Color,
		pub owner: T::AccountId,
	}

	#[derive(Clone, Encode, Decode, PartialEq, Copy, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	pub enum Color {
		Red,
		Yellow,
		Blue,
		Green
	}

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
	pub trait Config: frame_system::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type Currency: Currency<Self::AccountId>;

		#[pallet::constant]
		type MaximumOwned: Get<u32>;
	}

	#[pallet::storage]
	pub(super) type CollectiblesCount<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	pub(super) type HighestPrice<T> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	/// Maps the Collectible struct to the unique_id.
	#[pallet::storage]
	pub(super) type CollectibleMap<T: Config> = StorageMap<_, Twox64Concat, u64, Collectible<T>>;

	/// Track the collectibles owned by each account.
	#[pallet::storage]
	pub(super) type OwnerOfCollectibles<T: Config> = StorageMap<
		_,
		Twox64Concat,
		T::AccountId,
		BoundedVec<u64, T::MaximumOwned>,
		ValueQuery,
	>;

	#[pallet::error]
	pub enum Error<T> {
		DuplicateCollectible,
		MaximumCollectiblesOwned,
		BoundsOverflow,
		NoCollectible,
		NotOwner,
		TransferToSelf,
		BidPriceTooLow,
		NotForSale,
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		CollectibleCreated { collectible: u64, owner: T::AccountId },
		TransferSucceeded { from: T::AccountId, to: T::AccountId, collectible: u64 },
		PriceSet { collectible: u64, price: Option<BalanceOf<T>> },
		Sold { seller: T::AccountId, buyer: T::AccountId, collectible: u64, price: BalanceOf<T> },
	}

	#[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(_n: BlockNumberFor<T>) -> Weight {
            let collectibles_len = CollectiblesCount::<T>::get();
			let mut max_price = HighestPrice::<T>::get();
			for i in 0..collectibles_len {
				let collectible = CollectibleMap::<T>::get(&i).unwrap();
				if collectible.price > Some(max_price) {
					max_price = collectible.price.unwrap();
				}
			}
			HighestPrice::<T>::set(max_price);
			Weight::zero()
        }
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		pub fn create_collectible(origin: OriginFor<T>, to: T::AccountId) -> DispatchResult {
			ensure_signed(origin)?;
			let (collectible_gen_unique_id, color) = Self::gen_unique_id();
			Self::mint(&to, collectible_gen_unique_id, color)?;
			Ok(())
		}

		/// Transfer a collectible to another account.
		/// Any account that holds a collectible can send it to another account. 
		/// Transfer resets the price of the collectible, marking it not for sale.
		#[pallet::weight(0)]
		pub fn transfer(
			origin: OriginFor<T>,
			to: T::AccountId,
			unique_id: u64,
		) -> DispatchResult {
			let from = ensure_signed(origin)?;
			let collectible = CollectibleMap::<T>::get(&unique_id).ok_or(Error::<T>::NoCollectible)?;
			ensure!(collectible.owner == from, Error::<T>::NotOwner);
			Self::do_transfer(unique_id, to)?;
			Ok(())
		}

		/// Delete collection
		#[pallet::weight(0)]
		pub fn burn(origin: OriginFor<T>, unique_id: u64) -> DispatchResult {
			let from = ensure_signed(origin)?;
			let collectible = CollectibleMap::<T>::get(&unique_id).ok_or(Error::<T>::NoCollectible)?;
			ensure!(collectible.owner == from, Error::<T>::NotOwner);
			CollectibleMap::<T>::remove(&unique_id);
			Ok(())
		}

		/// Update the collectible price and write to storage.
		#[pallet::weight(0)]
		pub fn set_price(
			origin: OriginFor<T>,
			owner: T::AccountId,
			unique_id: u64,
			new_price: Option<BalanceOf<T>>,
		) -> DispatchResult {
			ensure_signed(origin)?;
			let mut collectible = CollectibleMap::<T>::get(&unique_id).unwrap();
			ensure!(collectible.owner == owner, Error::<T>::NotOwner);
			collectible.price = new_price;
			CollectibleMap::<T>::insert(&unique_id, collectible);
			Self::deposit_event(Event::PriceSet { collectible: unique_id, price: new_price });
			Ok(())
		}

		/// Buy a collectible. The bid price must be greater than or equal to the price
		/// set by the collectible owner.
		#[pallet::weight(0)]
		pub fn buy_collectible(
			origin: OriginFor<T>,
			buyer: T::AccountId,
			unique_id: u64,
			extra_fee: u128,
		) -> DispatchResult {
			ensure_signed(origin)?;
			Self::do_buy_collectible(unique_id, buyer, extra_fee)?;
			Ok(())
		}
	}

	// Pallet internal functions
	impl<T: Config> Pallet<T> {
		fn gen_unique_id() -> (u64, Color) {
			let collectibles_count = CollectiblesCount::<T>::get();
			
			if collectibles_count % 2 == 0 {
					(collectibles_count, Color::Red)
			} else {
					(collectibles_count, Color::Yellow)
			} 
		}

		// Function to mint a collectible
		pub fn mint(
			owner: &T::AccountId,
			unique_id: u64,
			color: Color,
		) -> Result<u64, DispatchError> {
			// Create a new object
			let collectible = Collectible::<T> { unique_id, price: None, color, owner: owner.clone() };
			
			// Check if the collectible exists in the storage map
			ensure!(!CollectibleMap::<T>::contains_key(&collectible.unique_id), Error::<T>::DuplicateCollectible);
			
			// Check that a new collectible can be created
			let count = CollectiblesCount::<T>::get();
			let new_count = count.checked_add(1).ok_or(Error::<T>::BoundsOverflow)?;
			
			// Append collectible to OwnerOfCollectibles map
			OwnerOfCollectibles::<T>::try_append(&owner, collectible.unique_id)
				.map_err(|_| Error::<T>::MaximumCollectiblesOwned)?;
			
			// Write new collectible to storage and update the count
			CollectibleMap::<T>::insert(collectible.unique_id, collectible);
			CollectiblesCount::<T>::put(new_count);
			
			// Deposit the "CollectibleCreated" event.
			Self::deposit_event(Event::CollectibleCreated { collectible: unique_id, owner: owner.clone() });
			
			// Returns the unique_id of the new collectible if this succeeds
			Ok(unique_id)
		}

		// Update storage to transfer collectible
		pub fn do_transfer(
			collectible_id: u64,
			to: T::AccountId,
		) -> DispatchResult {
			// Get the collectible
			let mut collectible = CollectibleMap::<T>::get(&collectible_id).ok_or(Error::<T>::NoCollectible)?;
			let from = collectible.owner;
			
			ensure!(from != to, Error::<T>::TransferToSelf);
			let mut from_owned = OwnerOfCollectibles::<T>::get(&from);
			
			// Remove collectible from list of owned collectible.
			if let Some(ind) = from_owned.iter().position(|&id| id == collectible_id) {
				from_owned.swap_remove(ind);
			} else {
				return Err(Error::<T>::NoCollectible.into())
			}
			// Add collectible to the list of owned collectibles.
			let mut to_owned = OwnerOfCollectibles::<T>::get(&to);
			to_owned.try_push(collectible_id).map_err(|_id| Error::<T>::MaximumCollectiblesOwned)?;
			
			// Transfer succeeded, update the owner and reset the price to `None`.
			collectible.owner = to.clone();
			collectible.price = None;

			// Write updates to storage
			CollectibleMap::<T>::insert(&collectible_id, collectible);
			OwnerOfCollectibles::<T>::insert(&to, to_owned);
			OwnerOfCollectibles::<T>::insert(&from, from_owned);
			
			Self::deposit_event(Event::TransferSucceeded { from, to, collectible: collectible_id });
			Ok(())
		}

		// An internal function for purchasing a collectible
		pub fn do_buy_collectible(
			unique_id: u64,
			to: T::AccountId,
			extra_fee: u128,
		) -> DispatchResult {
			// Get the collectible from the storage map
			let mut collectible = CollectibleMap::<T>::get(&unique_id).ok_or(Error::<T>::NoCollectible)?;
			let from = collectible.owner;
			ensure!(from != to, Error::<T>::TransferToSelf);
			let mut from_owned = OwnerOfCollectibles::<T>::get(&from);
			
			// Remove collectible from owned collectibles.
			if let Some(ind) = from_owned.iter().position(|&id| id == unique_id) {
				from_owned.swap_remove(ind);
			} else {
				return Err(Error::<T>::NoCollectible.into())
			}
			// Add collectible to owned collectible.
			let mut to_owned = OwnerOfCollectibles::<T>::get(&to);
			to_owned.try_push(unique_id).map_err(|_id| Error::<T>::MaximumCollectiblesOwned)?;
			// Mutating state with a balance transfer, so nothing is allowed to fail after this.
			if let Some(price) = collectible.price {
				//ensure!(bid_price >= price, Error::<T>::BidPriceTooLow);
				// Transfer the amount from buyer to seller
				let final_price = extra_fee + price.saturated_into::<u128>();
				T::Currency::transfer(&to, &from, final_price.saturated_into(), frame_support::traits::ExistenceRequirement::KeepAlive)?;
				// Deposit sold event
				Self::deposit_event(Event::Sold {
					seller: from.clone(),
					buyer: to.clone(),
					collectible: unique_id,
					price: final_price.saturated_into()
				});
			} else {
				return Err(Error::<T>::NotForSale.into())
			}

			// Transfer succeeded, update the collectible owner and reset the price to `None`.
			collectible.owner = to.clone();
			collectible.price = None;
			// Write updates to storage
			CollectibleMap::<T>::insert(&unique_id, collectible);
			OwnerOfCollectibles::<T>::insert(&to, to_owned);
			OwnerOfCollectibles::<T>::insert(&from, from_owned);
			Self::deposit_event(Event::TransferSucceeded { from, to, collectible: unique_id });
			Ok(())
		}
	}
}