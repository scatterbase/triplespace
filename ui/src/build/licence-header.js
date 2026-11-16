// The licence header of the built files (ADR 0077 §3; chapter 21 §5.3): every JavaScript
// and CSS file the build writes opens with a comment naming the licence and
// Special:Version, where a reader finds the licence texts and the source. The files are
// conveyed to every reader, so each says what it is under on its own.
//
// The comment opens with `/*!`, which minifiers keep. It is added to the files as
// written, after every other plugin is done with them (Vite writes a preload map at the
// top of an entry late): the text is the same in every build, so a file's hashed name
// still changes exactly when its code does.

/** The header, a comment valid in JavaScript and in CSS. */
export const LICENCE_HEADER =
	'/*! @license GPL-3.0-or-later. Triplespace; licence texts and source: Special:Version */\n';

/**
 * Whether a built file takes the header: JavaScript and CSS, by name.
 *
 * @param {string} fileName The file's name in the build.
 * @return {boolean} Whether it does.
 */
export function takesHeader( fileName ) {
	return /\.(?:js|mjs|css)$/.test( fileName );
}

/**
 * A file's text with the header, once.
 *
 * @param {string} text The file as built.
 * @return {string} The file as written.
 */
export function withHeader( text ) {
	return text.startsWith( LICENCE_HEADER ) ? text : LICENCE_HEADER + text;
}

/**
 * The Vite plugin: adds the header to every JavaScript and CSS file of the build, once
 * the build has written them.
 *
 * @param {Object} [fs] The file system to write through; Node's by default.
 * @return {Object} The plugin.
 */
export function licenceHeader( fs ) {
	return {
		name: 'triplespace-licence-header',
		async writeBundle( options, bundle ) {
			const { readFile, writeFile } = fs || await import( 'node:fs/promises' );
			const { join } = await import( 'node:path' );
			for ( const item of Object.values( bundle ) ) {
				if ( !takesHeader( item.fileName ) ) {
					continue;
				}
				const path = join( options.dir, item.fileName );
				const text = await readFile( path, 'utf8' );
				if ( !text.startsWith( LICENCE_HEADER ) ) {
					await writeFile( path, withHeader( text ) );
				}
			}
		}
	};
}
