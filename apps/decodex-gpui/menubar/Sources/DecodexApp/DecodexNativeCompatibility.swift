import Darwin
import Foundation

let decodexNativeClientABIVersion: UInt32 = 1
let decodexNativeArtifactCohort: UInt32 = 6

enum DecodexNativeCompatibility {
	private typealias Version = @convention(c) () -> UInt32

	private enum LoadError: Error {
		case unavailable
	}

	static func openLibrary(at libraryURL: URL) throws -> UnsafeMutableRawPointer {
		guard let image = dlopen(libraryURL.path, RTLD_NOW | RTLD_LOCAL) else {
			throw LoadError.unavailable
		}

		do {
			let abiVersion: Version = try symbol(
				image,
				named: "decodex_app_native_client_abi_version"
			)
			guard abiVersion() == decodexNativeClientABIVersion else {
				throw LoadError.unavailable
			}
			let artifactCohort: Version = try symbol(
				image,
				named: "decodex_app_native_client_artifact_cohort"
			)
			guard artifactCohort() == decodexNativeArtifactCohort else {
				throw LoadError.unavailable
			}
			return image
		} catch {
			dlclose(image)
			throw error
		}
	}

	private static func symbol<T>(
		_ image: UnsafeMutableRawPointer,
		named name: String
	) throws -> T {
		guard let value = dlsym(image, name) else {
			throw LoadError.unavailable
		}
		return unsafeBitCast(value, to: T.self)
	}
}
