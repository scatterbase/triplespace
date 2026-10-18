// triplespace-ui writes a palette's dark variant over Codex's dark values and the text-size
// modes from Codex's own (ADR 0086 §2, §5), from copies of the design-token files kept in
// crates/triplespace-ui/codex, since the Rust build does not need Node. They must be the
// copies of the version installed here.
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire( import.meta.url );
const here = dirname( fileURLToPath( import.meta.url ) );
const dist = join( dirname( require.resolve( '@wikimedia/codex-design-tokens/package.json' ) ), 'dist' );
const copies = join( here, '../../crates/triplespace-ui/codex' );

describe( 'codex tokens', () => {
	for ( const f of [ 'root', 'mode-dark', 'mode-small', 'mode-large', 'mode-x-large' ] ) {
		it( `crates/triplespace-ui/codex holds the installed theme-wikimedia-ui-${ f }.css`, () => {
			const name = `theme-wikimedia-ui-${ f }.css`;
			// eslint-disable-next-line security/detect-non-literal-fs-filename
			const copy = readFileSync( join( copies, name ), 'utf8' );
			// eslint-disable-next-line security/detect-non-literal-fs-filename
			expect( copy ).toBe( readFileSync( join( dist, name ), 'utf8' ) );
		} );
	}
} );
