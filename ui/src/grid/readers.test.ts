/* eslint-disable no-bitwise -- a ZIP file is written by hand, with its checksums */
import { describe, expect, it } from 'vitest';
import { readDelimited, readFile, readJsonLines, readQuickStatements, readXlsx } from './readers';

describe( 'delimited text', () => {
	// The same texts and rows as `tables_split_as_tabs_or_commas` in the server's table.rs.
	it( 'splits at tabs when the first line holds one', () => {
		expect( readDelimited( 'label\tP31\nSix\tQ5\n\nSeven\tQ6\n' ) ).toEqual( {
			kind: 'tsv',
			headers: [ 'label', 'P31' ],
			rows: [ [ 'Six', 'Q5' ], [ 'Seven', 'Q6' ] ],
		} );
		// No quoting in tab-separated text; the first non-empty line decides.
		expect( readDelimited( '\r\n \r\na\tb\r\n"x, y"\t z \r\n' ) ).toEqual( {
			kind: 'tsv',
			headers: [ 'a', 'b' ],
			rows: [ [ '"x, y"', 'z' ] ],
		} );
	} );

	it( 'splits at commas otherwise, with quoted cells', () => {
		const csv = '﻿label,description:de\r\n"Six, the number","eine ""Zahl"""\r\nSeven,\r\n';
		expect( readDelimited( csv ) ).toEqual( {
			kind: 'csv',
			headers: [ 'label', 'description:de' ],
			rows: [ [ 'Six, the number', 'eine "Zahl"' ], [ 'Seven', '' ] ],
		} );
		expect( readDelimited( 'a,b\n"two\nlines", x "y"\n,\n\n  \nlast' ).rows ).toEqual( [
			[ 'two\nlines', 'x "y"' ],
			[ 'last', '' ],
		] );
	} );

	it( 'pads and truncates rows to the header', () => {
		expect( readDelimited( 'a,b,c\n1\n1,2,3,4\n' ).rows ).toEqual( [ [ '1', '', '' ], [ '1', '2', '3' ] ] );
		expect( readDelimited( '' ) ).toEqual( { kind: 'csv', headers: [], rows: [] } );
	} );
} );

describe( 'JSON Lines', () => {
	it( 'takes the keys as columns in the order first seen', () => {
		const text = [
			'{"label":"Six","P31":"Q5","n":6,"even":true}',
			'',
			'{"P31":["Q5","a|b",null,7],"alias":null,"extra":{"a":[1]},"label":" Seven "}',
		].join( '\n' );
		expect( readJsonLines( text ) ).toEqual( {
			kind: 'jsonl',
			headers: [ 'label', 'P31', 'n', 'even', 'alias', 'extra' ],
			rows: [
				[ 'Six', 'Q5', '6', 'true', '', '' ],
				[ 'Seven', 'Q5|a\\|b|7', '', '', '', '{"a":[1]}' ],
			],
		} );
	} );

	it( 'names the line that is not an object', () => {
		expect( () => readJsonLines( '{"a":1}\n\n[1,2]\n' ) ).toThrow( /Line 3/ );
		expect( () => readJsonLines( '{"a":1}\n{"a":\n' ) ).toThrow( /Line 2/ );
		expect( () => readJsonLines( '"text"' ) ).toThrow( /Line 1/ );
	} );
} );

describe( 'QuickStatements', () => {
	it( 'reads CREATE and LAST lines as rows', () => {
		const text = [
			'# two people',
			'CREATE',
			'LAST\tLen\t"Douglas Adams"',
			'LAST\tDde\t"englischer Autor"',
			'LAST\tAen\t"DNA"',
			'LAST\tAen\t"Douglas N. Adams"',
			'LAST\tP31\tQ5\tS854\t"https://example.org/a"\tS813\t+2024-01-02T00:00:00Z/11',
			'LAST\tP569\t+1952-03-11T00:00:00Z/11\tP1319\t+1952-00-00T00:00:00Z/9 /* a summary */',
			'LAST\tP106\tQ36180',
			'LAST\tP106\tQ28389',
			'',
			'CREATE',
			'LAST\tLen\t"Apollo 11"',
			'LAST\tP569\t+1969-07-00T00:00:00Z/10',
			'LAST\tP571\t+1969-00-00T00:00:00Z/9',
			'LAST\tP572\t+1900-00-00T00:00:00Z/7',
			'LAST\tP1114\t+12',
			'LAST\tP1115\t3.5',
			'LAST\tP1476\ten:"One small step"',
			'LAST\tP625\t@45.5/-122.6',
			'LAST\tP1545\t"a|b"',
		].join( '\n' );
		expect( readQuickStatements( text, 'en' ) ).toEqual( {
			kind: 'quickstatements',
			headers: [
				'label', 'description:de', 'aliases', 'P31', 'P31/ref:P854', 'P31/ref:P813', 'P569',
				'P106', 'P571', 'P572', 'P1114', 'P1115', 'P1476', 'P625', 'P1545',
			],
			rows: [
				[
					'Douglas Adams', 'englischer Autor', 'DNA|Douglas N. Adams', 'Q5',
					'https://example.org/a', '2024-01-02', '1952-03-11', 'Q36180|Q28389',
					'', '', '', '', '', '', '',
				],
				[
					'Apollo 11', '', '', '', '', '', '1969-07', '', '1969', '+1900-00-00T00:00:00Z/7',
					'12', '3.5', 'One small step@en', '45.5, -122.6', 'a\\|b',
				],
			],
		} );
	} );

	it( 'writes the suffix of a language that is not the wiki\'s', () => {
		const read = readQuickStatements( 'CREATE\nLAST|Len|"Six"|\nLAST|Dde|"a|b"\nLAST|P31|Q5', 'de' );
		expect( read?.headers ).toEqual( [ 'label:en', 'description', 'P31' ] );
		expect( read?.rows ).toEqual( [ [ 'Six', 'a|b', 'Q5' ] ] );
	} );

	it( 'is null for anything else', () => {
		expect( readQuickStatements( 'label,P31\nSix,Q5\n', 'en' ) ).toBeNull();
		expect( readQuickStatements( '# nothing\n', 'en' ) ).toBeNull();
		// No CREATE before the first LAST.
		expect( readQuickStatements( 'LAST\tP31\tQ5\n', 'en' ) ).toBeNull();
		// An edit to an existing item.
		expect( readQuickStatements( 'CREATE\nLAST\tP31\tQ5\nQ42\tP31\tQ5\n', 'en' ) ).toBeNull();
		expect( readQuickStatements( 'CREATE\nLAST\tP31\tQ5\n-Q42\tP31\tQ5\n', 'en' ) ).toBeNull();
		// A command outside the subset, and a statement without its value.
		expect( readQuickStatements( 'CREATE\nLAST\tSenwiki\t"Six"\n', 'en' ) ).toBeNull();
		expect( readQuickStatements( 'CREATE\nLAST\tP31\n', 'en' ) ).toBeNull();
	} );
} );

/**
 * The CRC-32 of some bytes, as a ZIP records it.
 *
 * @param bytes The bytes
 * @return The checksum
 */
function crc32( bytes: Uint8Array ): number {
	let crc = 0xffffffff;
	for ( const byte of bytes ) {
		crc ^= byte;
		for ( let k = 0; k < 8; k++ ) {
			crc = ( crc >>> 1 ) ^ ( crc & 1 ? 0xedb88320 : 0 );
		}
	}
	return ( crc ^ 0xffffffff ) >>> 0;
}

/**
 * Deflates bytes with the platform's `CompressionStream`.
 *
 * @param bytes The bytes
 * @return The raw deflate data
 */
async function deflate( bytes: Uint8Array ): Promise<Uint8Array> {
	const stream = new CompressionStream( 'deflate-raw' );
	const writer = stream.writable.getWriter();
	const written = writer.write( new Uint8Array( bytes ) ).then( () => writer.close() );
	const reader = stream.readable.getReader();
	const out: number[] = [];
	for ( ;; ) {
		const { done, value } = await reader.read();
		if ( done ) {
			break;
		}
		out.push( ...value );
	}
	await written;
	return new Uint8Array( out );
}

/**
 * Writes a ZIP file by hand: each entry stored, or deflated when its name is in
 * `deflated`.
 *
 * @param files The entries' texts by name
 * @param deflated The names of the entries to deflate
 * @return The file
 */
async function zip( files: Record<string, string>, deflated: string[] = [] ): Promise<ArrayBuffer> {
	const encoder = new TextEncoder();
	const local: number[] = [];
	const central: number[] = [];
	const u16 = ( n: number ): number[] => [ n & 0xff, n >>> 8 ];
	const u32 = ( n: number ): number[] => [ ...u16( n & 0xffff ), ...u16( n >>> 16 ) ];
	let count = 0;
	for ( const [ name, text ] of Object.entries( files ) ) {
		const raw = encoder.encode( text );
		const method = deflated.includes( name ) ? 8 : 0;
		const data = method === 8 ? await deflate( raw ) : raw;
		const nameBytes = encoder.encode( name );
		// Version, flags (UTF-8 names), method, time, date, CRC, sizes, name and extra lengths.
		const fields = [
			...u16( 20 ), ...u16( 0x0800 ), ...u16( method ), ...u16( 0 ), ...u16( 0x21 ),
			...u32( crc32( raw ) ), ...u32( data.length ), ...u32( raw.length ),
			...u16( nameBytes.length ), ...u16( 0 ),
		];
		const offset = local.length;
		local.push( ...u32( 0x04034b50 ), ...fields, ...nameBytes, ...data );
		central.push(
			...u32( 0x02014b50 ), ...u16( 20 ), ...fields,
			// Comment length, disk, internal and external attributes, the local header.
			...u16( 0 ), ...u16( 0 ), ...u16( 0 ), ...u32( 0 ), ...u32( offset ), ...nameBytes,
		);
		count++;
	}
	const end = [
		...u32( 0x06054b50 ), ...u16( 0 ), ...u16( 0 ), ...u16( count ), ...u16( count ),
		...u32( central.length ), ...u32( local.length ), ...u16( 0 ),
	];
	return new Uint8Array( [ ...local, ...central, ...end ] ).buffer;
}

const CONTENT_TYPES = '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>';
const WORKBOOK = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Items" sheetId="7" r:id="rId3"/><sheet name="Other" sheetId="1" r:id="rId1"/></sheets></workbook>`;
const RELS = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
</Relationships>`;
const SHARED = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="5" uniqueCount="5">
<si><t>label</t></si>
<si><t xml:space="preserve"> P31 </t></si>
<si><r><rPr><b/></rPr><t>Tom &amp; </t></r><r><t xml:space="preserve">Jerry &lt;&#233;&#x1F600;&gt;</t></r><rPh sb="0" eb="1"><t>ignored</t></rPh></si>
<si/>
<si><t>description:de</t></si>
</sst>`;
const sheet = ( rows: string ): string => `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:E6"/>
<cols><col min="1" max="1" width="20"/></cols><sheetData>${ rows }</sheetData></worksheet>`;
const ITEMS = sheet( `
<row r="1" spans="1:5"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>4</v></c><c r="D1" t="inlineStr"><is><t>n</t></is></c><c r="E1" t="str"><f>"o"&amp;"k"</f><v>ok</v></c></row>
<row r="2"/>
<row r="3"><c r="A3" s="1"/><c r="C3" t="s"><v>3</v></c></row>
<row r="4"><c r="A4" t="s"><v>2</v></c><c r="D4"><v>0.10000000000000001</v></c><c r="E4" t="b"><v>1</v></c><c r="G4"><v>9</v></c></row>
<row r="6"><c r="B6" t="inlineStr"><is><r><t>Q</t></r><r><t>5</t></r></is></c><c r="D6"><v>12345678901234567890</v></c><c r="AA6" t="s"><v>0</v></c></row>
` );
const OTHER = sheet( '<row r="1"><c r="A1" t="inlineStr"><is><t>wrong sheet</t></is></c></row>' );

describe( 'workbooks', () => {
	it( 'reads the first sheet, by its relationship', async () => {
		const bytes = await zip( {
			'[Content_Types].xml': CONTENT_TYPES,
			'xl/workbook.xml': WORKBOOK,
			'xl/_rels/workbook.xml.rels': RELS,
			'xl/sharedStrings.xml': SHARED,
			'xl/worksheets/sheet1.xml': OTHER,
			'xl/worksheets/sheet2.xml': ITEMS,
		}, [ 'xl/sharedStrings.xml', 'xl/worksheets/sheet2.xml' ] );
		expect( await readXlsx( bytes ) ).toEqual( {
			kind: 'xlsx',
			headers: [ 'label', 'P31', 'description:de', 'n', 'ok' ],
			rows: [
				[ 'Tom & Jerry <é😀>', '', '', '0.1', '1' ],
				[ '', 'Q5', '', '12345678901234567890', '' ],
			],
		} );
	} );

	it( 'falls back to sheet1.xml, and places cells without references in order', async () => {
		const bytes = await zip( {
			'xl/worksheets/sheet1.xml': sheet(
				'<x:row><x:c t="inlineStr"><x:is><x:t>a</x:t></x:is></x:c><x:c t="inlineStr"><x:is><x:t>b</x:t></x:is></x:c></x:row>' +
				'<row><c><v>1</v></c><c><v>2</v></c><c><v>3</v></c></row>',
			),
		} );
		expect( await readXlsx( bytes ) ).toEqual( { kind: 'xlsx', headers: [ 'a', 'b' ], rows: [ [ '1', '2' ] ] } );
	} );

	it( 'says when the bytes are not a workbook', async () => {
		await expect( readXlsx( new TextEncoder().encode( 'label,P31\n' ).buffer ) ).rejects.toThrow( /not a ZIP/ );
		await expect( readXlsx( new ArrayBuffer( 0 ) ) ).rejects.toThrow( /not a ZIP/ );
		await expect( readXlsx( await zip( { 'word/document.xml': '<w:document/>' } ) ) )
			.rejects.toThrow( /no worksheet/ );
		// A deflated entry whose data is not deflate.
		const bytes = new Uint8Array( await zip( { 'xl/worksheets/sheet1.xml': ITEMS } ) );
		bytes[ 8 ] = 8;
		const directory = bytes.findIndex(
			( _, i ) => bytes[ i ] === 0x50 && bytes[ i + 1 ] === 0x4b && bytes[ i + 2 ] === 1,
		);
		bytes[ directory + 10 ] = 8;
		await expect( readXlsx( bytes.buffer ) ).rejects.toThrow( /sheet1\.xml is damaged/ );
	} );
} );

describe( 'files', () => {
	it( 'are read by extension', async () => {
		const xlsx = new File( [ await zip( { 'xl/worksheets/sheet1.xml': ITEMS } ) ], 'Items.XLSX' );
		expect( ( await readFile( xlsx, 'en' ) ).kind ).toBe( 'xlsx' );
		expect( ( await readFile( new File( [ '{"a":1}\n' ], 'a.ndjson' ), 'en' ) ).kind ).toBe( 'jsonl' );
		// Text that would otherwise read as a one-column table.
		await expect( readFile( new File( [ 'a\nb\n' ], 'a.jsonl' ), 'en' ) ).rejects.toThrow( /Line 1/ );
		expect( ( await readFile( new File( [ 'CREATE\nLAST\tLen\t"Six"\n' ], 'a.qs' ), 'en' ) ).headers )
			.toEqual( [ 'label' ] );
		await expect( readFile( new File( [ 'label,P31\nSix,Q5\n' ], 'a.qs' ), 'en' ) )
			.rejects.toThrow( /QuickStatements/ );
	} );

	it( 'are otherwise read by their contents', async () => {
		const kind = async ( text: string, name: string ): Promise<string> => (
			await readFile( new File( [ text ], name ), 'en' )
		).kind;
		expect( await kind( 'CREATE\nLAST\tLen\t"Six"\n', 'commands.txt' ) ).toBe( 'quickstatements' );
		expect( await kind( '\n {"label":"Six"}\n', 'upload' ) ).toBe( 'jsonl' );
		expect( await kind( 'label,P31\nSix,Q5\n', 'items.csv' ) ).toBe( 'csv' );
		expect( await kind( 'label\tP31\nSix\tQ5\n', 'items.csv' ) ).toBe( 'tsv' );
	} );

	it( 'are refused when empty', async () => {
		await expect( readFile( new File( [ ' \n\n' ], 'items.csv' ), 'en' ) ).rejects.toThrow( /no header row/ );
		await expect( readFile( new File( [ 'CREATE\n' ], 'a.qs' ), 'en' ) ).rejects.toThrow( /no header row/ );
	} );
} );
