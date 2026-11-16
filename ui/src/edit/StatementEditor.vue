<!-- eslint-disable vue/no-mutating-props -->
<template>
	<div class="ts-statement-editor" :class="{ 'ts-statement-editor--removed': row.removed }">
		<template v-if="row.removed">
			<p class="ts-statement-editor__removed">
				{{ t( 'ts-edit-removed' ) }}
			</p>
			<CdxButton
				type="button"
				:disabled="disabled"
				@click="row.removed = false">
				{{ t( 'ts-edit-undo-remove' ) }}
			</CdxButton>
		</template>
		<template v-else>
			<SnakEditor
				class="ts-statement-editor__main"
				:row="row.main"
				:legend="`${ propertyLabel } ${ position }`"
				:data="data"
				:api="api"
				:labels="labels"
				:disabled="disabled"
			>
				<CdxSelect
					v-model:selected="row.rank"
					class="ts-statement-editor__rank"
					:menu-items="ranks"
					:disabled="disabled"
					:aria-label="`${ t( 'ts-edit-rank' ) }, ${ propertyLabel } ${ position }`"
				/>
				<CdxButton
					type="button"
					action="destructive"
					weight="quiet"
					:disabled="disabled"
					@click="row.removed = true"
				>
					{{ t( 'ts-edit-remove-value' ) }}
				</CdxButton>
			</SnakEditor>

			<div class="ts-statement-editor__section">
				<p class="ts-statement-editor__heading">
					{{ t( 'ts-edit-qualifiers' ) }}
				</p>
				<SnakEditor
					v-for="( q, i ) in row.qualifiers"
					:key="i"
					:row="q"
					:legend="name( q.property )"
					:data="data"
					:api="api"
					:labels="labels"
					:disabled="disabled"
				>
					<CdxButton
						type="button"
						weight="quiet"
						:disabled="disabled"
						:aria-label="t( 'ts-edit-remove-snak', name( q.property ) )"
						@click="row.qualifiers.splice( i, 1 )"
					>
						{{ t( 'ts-edit-remove' ) }}
					</CdxButton>
				</SnakEditor>
				<PropertyPicker
					:data="data"
					:api="api"
					:action="t( 'ts-edit-add-qualifier' )"
					:disabled="disabled"
					@chosen="addQualifier"
				/>
			</div>

			<div class="ts-statement-editor__section">
				<p class="ts-statement-editor__heading">
					{{ t( 'ts-edit-references' ) }}
				</p>
				<div
					v-for="( r, ri ) in row.references"
					:key="ri"
					class="ts-statement-editor__reference"
				>
					<SnakEditor
						v-for="( s, si ) in r.snaks"
						:key="si"
						:row="s"
						:legend="name( s.property )"
						:data="data"
						:api="api"
						:labels="labels"
						:disabled="disabled"
					>
						<CdxButton
							type="button"
							weight="quiet"
							:disabled="disabled"
							:aria-label="t( 'ts-edit-remove-snak', name( s.property ) )"
							@click="r.snaks.splice( si, 1 )"
						>
							{{ t( 'ts-edit-remove' ) }}
						</CdxButton>
					</SnakEditor>
					<div class="ts-statement-editor__actions">
						<PropertyPicker
							:data="data"
							:api="api"
							:action="t( 'ts-edit-add-reference-snak' )"
							:disabled="disabled"
							@chosen="( p, dt, l ) => addToReference( ri, p, dt, l )"
						/>
						<CdxButton
							type="button"
							action="destructive"
							weight="quiet"
							:disabled="disabled"
							@click="row.references.splice( ri, 1 )"
						>
							{{ t( 'ts-edit-remove-reference' ) }}
						</CdxButton>
					</div>
				</div>
				<PropertyPicker
					:data="data"
					:api="api"
					:action="t( 'ts-edit-add-reference' )"
					:disabled="disabled"
					@chosen="addReference"
				/>
			</div>
		</template>
	</div>
</template>

<script setup lang="ts">
// One statement in the group editor: its value and rank, its qualifiers and its
// sources, each of which can be added or removed. The row is the group editor's
// reactive state, which this component edits in place.
/* eslint-disable vue/no-mutating-props */
import { computed } from 'vue';
import { CdxButton, CdxSelect } from '@wikimedia/codex';
import type { Api } from './api';
import { msg } from './data';
import type { EditData } from './data';
import PropertyPicker from './PropertyPicker.vue';
import SnakEditor from './SnakEditor.vue';
import { newSnak } from './statements';
import type { StatementRow } from './statements';

const props = defineProps<{
	row: StatementRow;
	position: number;
	propertyLabel: string;
	data: EditData;
	api: Api;
	labels: Record<string, string>;
	disabled?: boolean;
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );
const name = ( id: string ): string => props.labels[ id ] ?? id;
const ranks = computed( () => [
	{ value: 'preferred', label: t( 'ts-edit-rank-preferred' ) },
	{ value: 'normal', label: t( 'ts-edit-rank-normal' ) },
	{ value: 'deprecated', label: t( 'ts-edit-rank-deprecated' ) }
] );

function addQualifier( property: string, datatype: string, label: string ): void {
	props.labels[ property ] = label;
	props.row.qualifiers.push( newSnak( property, datatype ) );
}

function addReference( property: string, datatype: string, label: string ): void {
	props.labels[ property ] = label;
	props.row.references.push( { snaks: [ newSnak( property, datatype ) ] } );
}

function addToReference( index: number, property: string, datatype: string, label: string ): void {
	props.labels[ property ] = label;
	props.row.references[ index ].snaks.push( newSnak( property, datatype ) );
}
</script>
