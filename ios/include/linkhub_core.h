/*
 * linkhub_core.h — C ABI for the Rust core (core-rs/src/ios_bridge.rs).
 *
 * Every function returns a heap-allocated, NUL-terminated UTF-8 JSON string
 * (except linkhub_free_string). The caller OWNS the returned pointer and MUST
 * release it with linkhub_free_string(). Passing NULL for a string argument is
 * tolerated (treated as empty). Errors are reported in-band as JSON of the form
 * {"error":"..."} (or {"success":false,"error":"..."} for confirm_pairing).
 *
 * This header is consumed by Swift through include/module.modulemap as the
 * `LinkHubCoreFFI` Clang module: `import LinkHubCoreFFI`.
 */
#ifndef LINKHUB_CORE_H
#define LINKHUB_CORE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Identity */
char *linkhub_generate_identity(const char *device_name);
char *linkhub_restore_identity(const char *signing_key_hex,
                               const char *static_dh_key_hex,
                               const char *device_name);

/* Pairing */
char *linkhub_generate_pairing_payload(const char *device_id,
                                       const char *device_name,
                                       const char *public_key,
                                       const char *dh_public_key,
                                       uint64_t ttl_seconds);
char *linkhub_parse_pairing_payload(const char *identity_json,
                                    const char *payload);
char *linkhub_confirm_pairing(const char *identity_json, const char *payload,
                              const char *confirmation_code);

/* Lifecycle — release any string returned by the functions above. */
void linkhub_free_string(char *ptr);

#ifdef __cplusplus
}
#endif

#endif /* LINKHUB_CORE_H */
