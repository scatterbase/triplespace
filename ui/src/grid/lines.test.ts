// The lines of the statements on every item and of the reference on every statement
// (ADR 0106 §6, A10; 21 §6.7), as the lists beside the grid read and write them.
import { describe, expect, it } from 'vitest';
import { linesOf, textOfLines } from './lines';

describe( 'the lines beside the grid', () => {
	it( 'reads a property with its value, or the column it reads', () => {
		expect( linesOf( 'p31: Q5\n\nP854:{ source url }\nP2:a:b\n' ) ).toEqual( [
			{ property: 'P31', value: 'Q5', column: '' },
			{ property: 'P854', value: '', column: 'source url' },
			{ property: 'P2', value: 'a:b', column: '' },
		] );
	} );

	it( 'shows a line that is not one with no property', () => {
		expect( linesOf( 'nonsense\nP5:\nP6:{}' ) ).toEqual( [
			{ property: '', value: 'nonsense', column: '' },
			{ property: '', value: 'P5:', column: '' },
			{ property: 'P6', value: '{}', column: '' },
		] );
	} );

	it( 'writes what it read, and leaves out a line that is not finished', () => {
		const text = 'P31:Q5\nP854:{source url}';
		expect( textOfLines( linesOf( text ) ) ).toBe( text );
		expect( textOfLines( [
			{ property: 'P31', value: ' Q5 ', column: '' },
			{ property: '', value: 'x', column: '' },
			{ property: 'P2', value: '', column: '' },
		] ) ).toBe( 'P31:Q5' );
	} );
} );
