//! CoreAudio's default input is a policy, not a permanently selected device ID.
use block2::RcBlock;
use objc2_core_foundation::{CFRetained, CFString};
use std::{
	ffi::c_void,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

// CoreAudio/AudioHardware.h. Keep the three-field C address and exported signatures exact.
#[repr(C)]
struct Address {
	selector: u32,
	scope: u32,
	element: u32,
}
const INPUT: Address = Address {
	selector: u32::from_be_bytes(*b"dIn "),
	scope: u32::from_be_bytes(*b"glob"),
	element: 0,
};
const OUTPUT: Address = Address {
	selector: u32::from_be_bytes(*b"dOut"),
	scope: u32::from_be_bytes(*b"glob"),
	element: 0,
};
type Listener = dyn Fn(u32, *const c_void);
#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
	fn AudioObjectGetPropertyData(
		object: u32,
		address: *const Address,
		qualifier_size: u32,
		qualifier: *const c_void,
		size: *mut u32,
		data: *mut c_void,
	) -> i32;
	fn AudioObjectAddPropertyListenerBlock(
		object: u32,
		address: *const Address,
		queue: *mut c_void,
		block: *mut block2::DynBlock<Listener>,
	) -> i32;
	fn AudioObjectRemovePropertyListenerBlock(
		object: u32,
		address: *const Address,
		queue: *mut c_void,
		block: *mut block2::DynBlock<Listener>,
	) -> i32;
}
pub(super) struct Route {
	requested: u32,
	pub(super) selected: u32,
	output: u32,
	changed: Arc<AtomicBool>,
	listener: RcBlock<Listener>,
}
impl Route {
	pub(super) fn requested(&self) -> u32 {
		self.requested
	}

	pub(super) fn input_uid(&self) -> Result<CFRetained<CFString>, ()> {
		let address = Address { selector: u32::from_be_bytes(*b"uid "), ..INPUT };
		let mut value: *mut CFString = std::ptr::null_mut();
		let mut size = size_of_val(&value) as u32;
		let status = unsafe {
			AudioObjectGetPropertyData(
				self.selected,
				&address,
				0,
				std::ptr::null(),
				&mut size,
				(&raw mut value).cast(),
			)
		};
		if status != 0 {
			return Err(());
		}
		// CoreAudio returns a retained CFString for the device UID.
		Ok(unsafe { CFRetained::from_raw(std::ptr::NonNull::new(value).ok_or(())?) })
	}

	pub(super) fn new(requested: u32) -> Result<Self, ()> {
		let changed = Arc::new(AtomicBool::new(false));
		let listener: RcBlock<Listener> = {
			let dirty = changed.clone();
			let callback = RcBlock::new(move |_: u32, _: *const c_void| {
				dirty.store(true, Ordering::Release);
			});
			let mut registered = Vec::new();
			for address in [(requested == 0).then_some(&INPUT), Some(&OUTPUT)].into_iter().flatten()
			{
				if unsafe {
					AudioObjectAddPropertyListenerBlock(
						1,
						address,
						std::ptr::null_mut(),
						RcBlock::as_ptr(&callback),
					)
				} != 0
				{
					for address in registered {
						unsafe {
							AudioObjectRemovePropertyListenerBlock(
								1,
								address,
								std::ptr::null_mut(),
								RcBlock::as_ptr(&callback),
							);
						}
					}
					return Err(());
				}
				registered.push(address);
			}
			callback
		};
		let mut route = Self { requested, selected: requested, output: 0, changed, listener };
		route.selected = route.resolve()?;
		route.output = read(&OUTPUT)?;
		Ok(route)
	}

	fn resolve(&self) -> Result<u32, ()> {
		if self.requested != 0 {
			return Ok(self.requested);
		}
		read(&INPUT)
	}

	pub(super) fn update(&mut self) -> Result<(bool, bool), ()> {
		if !self.changed.swap(false, Ordering::AcqRel) {
			return Ok((false, false));
		}
		let id = self.resolve()?;
		let changed = id != self.selected;
		self.selected = id;
		let next_output = read(&OUTPUT)?;
		let output_changed = next_output != self.output;
		self.output = next_output;
		Ok((changed, output_changed))
	}
}
impl Drop for Route {
	fn drop(&mut self) {
		for address in
			[(self.requested == 0).then_some(&INPUT), Some(&OUTPUT)].into_iter().flatten()
		{
			unsafe {
				AudioObjectRemovePropertyListenerBlock(
					1,
					address,
					std::ptr::null_mut(),
					RcBlock::as_ptr(&self.listener),
				);
			}
		}
	}
}

fn read(address: &Address) -> Result<u32, ()> {
	let mut id = 0_u32;
	let mut size = size_of::<u32>() as u32;
	let status = unsafe {
		AudioObjectGetPropertyData(1, address, 0, std::ptr::null(), &mut size, (&raw mut id).cast())
	};
	if status != 0 || id == 0 {
		return Err(());
	}
	Ok(id)
}
